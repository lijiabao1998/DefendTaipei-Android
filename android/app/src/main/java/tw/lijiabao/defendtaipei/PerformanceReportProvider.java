package tw.lijiabao.defendtaipei;

import androidx.core.content.FileProvider;

/** Dedicated FileProvider subclass for reliable initialization across devices. */
public final class PerformanceReportProvider extends FileProvider {
    public PerformanceReportProvider() {
        super();
    }
}
