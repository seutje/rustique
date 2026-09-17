use serde::{Deserialize, Serialize};

use crate::{ActiveModulation, ModulationCombine, ModulationTarget};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationTrackV1 {
    pub target: ModulationTarget,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub keyframes: Vec<AutomationKeyframeV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationKeyframeV1 {
    pub time_seconds: f32,
    pub value: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneMarkerV1 {
    pub time_seconds: f32,
    pub label: String,
}

const fn enabled() -> bool {
    true
}

#[must_use]
pub fn evaluate_automation(
    tracks: &[AutomationTrackV1],
    time_seconds: f32,
) -> Vec<ActiveModulation> {
    tracks
        .iter()
        .filter(|track| track.enabled)
        .filter_map(|track| {
            let first = *track.keyframes.first()?;
            let value = track
                .keyframes
                .windows(2)
                .find_map(|pair| {
                    let [left, right] = pair else {
                        return None;
                    };
                    if time_seconds < left.time_seconds || time_seconds > right.time_seconds {
                        return None;
                    }
                    let span = right.time_seconds - left.time_seconds;
                    let blend = if span <= f32::EPSILON {
                        1.0
                    } else {
                        (time_seconds - left.time_seconds) / span
                    };
                    Some(left.value + (right.value - left.value) * blend.clamp(0.0, 1.0))
                })
                .unwrap_or_else(|| {
                    if time_seconds <= first.time_seconds {
                        first.value
                    } else {
                        track
                            .keyframes
                            .last()
                            .map_or(first.value, |keyframe| keyframe.value)
                    }
                });
            Some(ActiveModulation {
                target: track.target,
                source_value: value,
                output_value: value,
                combine: ModulationCombine::Replace,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automation_interpolates_and_clamps_to_endpoints() {
        let track = AutomationTrackV1 {
            target: ModulationTarget::CameraFov,
            enabled: true,
            keyframes: vec![
                AutomationKeyframeV1 {
                    time_seconds: 1.0,
                    value: 0.0,
                },
                AutomationKeyframeV1 {
                    time_seconds: 3.0,
                    value: 1.0,
                },
            ],
        };
        let value_at =
            |time| evaluate_automation(std::slice::from_ref(&track), time)[0].output_value;
        assert!(value_at(0.0).abs() < f32::EPSILON);
        assert!((value_at(2.0) - 0.5).abs() < f32::EPSILON);
        assert!((value_at(4.0) - 1.0).abs() < f32::EPSILON);
    }
}
