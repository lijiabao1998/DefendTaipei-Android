# Android 測試版更新與存檔搬移

一般更新直接安裝新 APK 即可；相同簽章會保留原有資料。早期 CI 每次產生不同的臨時 debug 簽章，Android 可能拒絕直接更新。現在改用[固定開發簽章](TEST-SIGNING.md)，從舊的臨時簽章轉過來時，可以用這個工具先保存進度，再完成一次存檔搬移。

工具：[`scripts/update-android-test-build.py`](../scripts/update-android-test-build.py)。只需要 **Python 3.10 以上、Android Platform Tools 的 `adb`**，不需要安裝 Android Studio 或 aapt。它只接受套件名稱為 `tw.lijiabao.defendtaipei`、允許 `run-as` 存取的 debug APK。

## 更新步驟

1. 在手機的舊版遊戲按「儲存」。接下來直到工具完成前，先不要重新開啟遊戲。
2. 開啟手機的 USB 偵錯，用 USB 接到電腦，接受手機上的電腦授權提示。
3. 在電腦執行 `adb devices`，確認裝置狀態是 `device`。
4. 在 repo 目錄執行，將 APK 路徑換成下載的檔案：

```sh
python3 scripts/update-android-test-build.py --apk "/path/to/DefendTaipei-arm64-debug.apk"
```

Windows 如果使用 `python` 啟動 Python，將範例中的 `python3` 改成 `python`。

工具會先執行普通的 `adb install -r -t`。新安裝、相同簽章的更新都走這條路，**不清除資料、不移除 App**。成功後開啟遊戲，有原有存檔就選「繼續」。

如果它明確指出這個遊戲的簽章不同，執行以下指令。`--backup-dir` 請選電腦上會保留的資料夾，不要使用暫存目錄；省略時使用目前使用者家目錄的 `DefendTaipei-backups`：

```sh
python3 scripts/update-android-test-build.py --apk "/path/to/DefendTaipei-arm64-debug.apk" --backup-dir "/path/to/DefendTaipei-backups" --migrate
```

**`--migrate` 允許一次跨簽章重裝。** 工具仍會先嘗試正常更新；只有 Android 回覆 `INSTALL_FAILED_UPDATE_INCOMPATIBLE` 且明確指向本遊戲時，才會啟用搬移程序。其他安裝錯誤，例如空間不足、APK 不相容或降版限制，都不會觸發移除。

搬移順序如下：

1. 停止遊戲，讀取 App 私有目錄的 `files/defendtaipei-native-v1.json`。
2. 檢查 JSON、版本 1、角色、時間、人物狀態、背包、避難所、世界進度等必要欄位。
3. 在電腦寫入**唯一名稱**的原始 JSON 和 `.json.sha256` 校驗檔，flush/fsync 後重新讀取驗證。POSIX 系統也同步新建資料夾的父目錄項目。
4. 以上全部成功後，才移除舊測試版並安裝新 APK。
5. 將原始位元組寫入新 App 的私有暫存檔，核對 SHA-256，再原子替換正式存檔，最後再次讀回核對。

工具不會改寫或重新排版 JSON，不會把存檔內容印到終端。它只列出備份路徑與 SHA-256。完整遊戲進度仍由遊戲的「繼續」功能載入並執行遊戲端驗證；工具的檢查不是遊戲世界資料驗證器的替代品。

沒有存檔、存檔不完整、無法讀取 debug 私有資料、備份無法寫入或校驗不符時，工具會停止，**不移除舊 App**。如果舊版尚未建立存檔，先開一局並手動儲存，再重跑。

## 連接多台裝置時

工具不會猜測要操作哪一台。只有一台已授權、在線的裝置時會自動選取；多台時必須用 `adb devices` 顯示的完整序號指定：

```sh
python3 scripts/update-android-test-build.py --apk "/path/to/new.apk" --serial "YOUR_DEVICE_SERIAL" --backup-dir "/path/to/backups" --migrate
```

指定的裝置不在線或尚未授權時，工具直接停止，不會改用另一台。

## 單獨備份

不安裝任何 APK，也可以先備份目前的進度：

```sh
python3 scripts/update-android-test-build.py backup --backup-dir "/path/to/backups" --serial "YOUR_DEVICE_SERIAL"
```

請一起保留工具輸出的 `.json` 與相鄰的 `.json.sha256` 檔案。每次備份都產生新檔，不覆寫之前的備份。Unix 上新檔使用只允許目前使用者讀寫的權限；Windows 使用所選資料夾的存取權限並同步檔案內容。

## 新 APK 安裝失敗後恢復

如果舊版已移除，但新 APK 安裝或存檔復原失敗，工具會明確顯示失敗階段、保留已驗證的電腦備份，並列出填好實際路徑與裝置序號的恢復指令。**它不會宣稱已回復舊版。**

修正錯誤原因，例如釋放手機空間、重新接好 USB，或下載完整的 debug APK，然後執行工具印出的指令。形式如下：

```sh
python3 scripts/update-android-test-build.py restore --apk "/path/to/new.apk" --backup "/path/to/backups/defendtaipei-save-TIMESTAMP-ID.json" --serial "YOUR_DEVICE_SERIAL"
```

`restore` 會先核對本機 JSON 與校驗檔；提供 `--apk` 時嘗試安裝，再停止遊戲並恢復存檔。**這個命令不會移除任何 App。** 若新 APK 已正確安裝，可省略 `--apk`。

`restore` 會用選取的備份覆蓋目前那一格存檔。如果已經在新版玩過一段時間，先用 `backup` 保存新的進度，再選擇要恢復的備份。完成驗證前不要開啟遊戲；成功後選「繼續」。

## 開發者驗證

以下測試全部使用假的 ADB runner 和暫存資料夾，不會連線、安裝或移除任何實際裝置上的 App：

```sh
python3 -m unittest discover -s scripts/tests -p 'test_android_update.py' -v
```

測試覆蓋明確遷移授權、精確錯誤與套件判斷、裝置歧義、錯誤 APK、無存檔或壞存檔時禁止移除、備份先於移除、磁碟同步失敗、原始位元組恢復、暫存／最終校驗失敗、重裝失敗後保留備份，以及恢復命令。

### Android 模擬器的實際更新驗收

CI 在遊戲觸控驗收產生實際存檔後，接著執行：

```sh
UPDATE_SMOKE_OUT="$GITHUB_WORKSPACE/emulator-update-smoke" bash scripts/emulator-update-smoke.sh "$GITHUB_WORKSPACE/DefendTaipei-x86_64-debug.apk"
```

這個腳本只接受 `emulator-數字` 序號且 `ro.kernel.qemu=1` 的裝置；多台在線時必須明確設定 `ANDROID_SERIAL`。它使用同一份使用者更新工具，先驗證相同簽章更新，再用臨時 CI 測試簽章簽署 APK 副本，實際執行跨簽章遷移，最後遷回提供的固定簽章 APK。三個檢查點都要求私有存檔與原始備份**每個位元組相同**，兩次遷移也必須各產生一份有效備份與 SHA-256。原始 APK 不得被改寫。

輸出資料夾保存 JSON 驗收記錄、操作日誌、讀回的存檔及已驗證備份。臨時私鑰、密碼及替代 APK 放在輸出目錄之外，離開腳本時刪除，不上傳為驗收產物。執行環境需要 Python 3、ADB、JDK `keytool` 與 Android SDK `apksigner`；可用 `APKSIGNER` 或 `ANDROID_SDK_ROOT`／`ANDROID_HOME` 指定簽署工具。這項驗收確認安裝與存檔搬移，不代表已驗證實體手機 USB 穩定度，或遷移後再按「繼續」遊玩。
