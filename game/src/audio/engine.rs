//! One bounded engine voice for the nearest live armored vehicle. The source
//! armor.ts uses 38 Hz sawtooth + 57 Hz square through a 220 Hz low-pass filter.
//! Gain and all lifecycle rules go through the normal native audio mixer.
use super::*;

#[derive(Component)]
pub(super) struct EngineVoice;

/// Explicit real-output QA fixture. Normal games do not insert this resource.
#[derive(Resource)]
pub(crate) struct ArmorEngineFixture(pub Option<f32>);

pub(super) fn target_gain(distance: f32) -> f32 {
    if distance.is_finite() && (0.0..90.0).contains(&distance) {
        0.12 * (1.0 - distance / 90.0)
    } else {
        0.0
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    time: Res<Time>,
    rt: Res<Runtime>,
    settings: Res<AudioSettings>,
    control: Res<AudioScheduleControl>,
    fixture: Option<Res<ArmorEngineFixture>>,
    fleet: Option<Res<crate::vehicles::VehicleFleet>>,
    armor: Option<Res<crate::combat::armor::ArmorFleet>>,
    player: Query<&Transform, With<crate::player::Player>>,
    clips: Res<Clips>,
    assets: Res<Assets<AudioSource>>,
    mut voices: Query<(Entity, &mut AudioVoice, Option<&AudioSink>), With<EngineVoice>>,
    mut diagnostics: ResMut<AudioDiagnostics>,
) {
    let distance = if let Some(d) = fixture.as_ref().and_then(|f| f.0) {
        d
    } else if control.automatic {
        let driven = fleet
            .as_ref()
            .and_then(|f| f.current())
            .is_some_and(|v| v.spec().armored && v.fuel > 0.0 && v.engine >= 0.1);
        if driven {
            0.0
        } else {
            player.single().ok().map_or(f32::INFINITY, |p| {
                armor.as_ref().map_or(f32::INFINITY, |a| {
                    a.units
                        .iter()
                        .filter(|a| a.active())
                        .map(|a| a.position().distance(p.translation.xz()))
                        .fold(f32::INFINITY, f32::min)
                })
            })
        }
    } else {
        f32::INFINITY
    };
    let enabled = rt.foreground && rt.world_active && settings.ambience_gain() > 0.0;
    let target = if enabled { target_gain(distance) } else { 0.0 };
    let mut retained = false;
    for (entity, mut voice, sink) in &mut voices {
        if !enabled || retained {
            if let Some(sink) = sink {
                sink.stop();
            }
            commands.entity(entity).despawn();
            continue;
        }
        voice.base_gain +=
            (target - voice.base_gain) * (1.0 - (-time.delta_secs().min(1.0) / 0.3).exp());
        if target == 0.0 && voice.base_gain < 0.0001 {
            if let Some(sink) = sink {
                sink.stop();
            }
            commands.entity(entity).despawn();
        } else {
            retained = true;
        }
    }
    if target <= 0.0 || retained || !clips.loaded("armor_engine.ogg", &assets) {
        return;
    }
    let Some((handle, _)) = clips.get("armor_engine.ogg") else {
        return;
    };
    commands.spawn((
        EngineVoice,
        AudioVoice {
            cue: "armor_engine".into(),
            category: AudioCategory::Ambience,
            base_gain: target,
            remaining: 3600.0,
            is_preview: false,
            recorded: false,
            scope: Scope::World,
            looping: true,
        },
        AudioPlayer::new(handle.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
    ));
    *diagnostics
        .started
        .entry("armor_engine".into())
        .or_default() += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_gain_follows_original_range_and_never_accepts_nonfinite_values() {
        assert_eq!(target_gain(0.0), 0.12);
        assert!((target_gain(45.0) - 0.06).abs() < 1e-6);
        for d in [-1.0, 90.0, 100.0, f32::NAN, f32::INFINITY] {
            assert_eq!(target_gain(d), 0.0);
        }
    }
}
