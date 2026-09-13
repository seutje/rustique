//! Deterministic, renderer-independent creative editing operations.

use std::collections::BTreeMap;

use crate::{PresetOverridesV1, ProjectError, ProjectV1, VisualPresetV1};

/// Creates bounded macro overrides from a stored seed and mutation amount.
/// An amount of zero returns the preset defaults; one can use the full range.
///
/// # Errors
///
/// Returns an error when `amount` is not finite or outside `0..=1`.
pub fn randomize_preset_macros(
    preset: &VisualPresetV1,
    seed: u64,
    amount: f32,
) -> Result<PresetOverridesV1, ProjectError> {
    validate_amount(amount)?;
    let macros = preset
        .macros
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            let random = unit_hash(seed, index as u64);
            let target = parameter.minimum + (parameter.maximum - parameter.minimum) * random;
            (
                parameter.id.clone(),
                parameter.default + (target - parameter.default) * amount,
            )
        })
        .collect();
    Ok(PresetOverridesV1 { macros })
}

/// Morphs matching macro values between two presets and applies the result.
/// Discrete simulation topology switches at the midpoint; shared numeric scene
/// properties are interpolated continuously.
///
/// # Errors
///
/// Returns an error for an invalid amount, preset, or resulting project.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub fn morph_presets(
    left: &VisualPresetV1,
    right: &VisualPresetV1,
    amount: f32,
    project: &mut ProjectV1,
) -> Result<PresetOverridesV1, ProjectError> {
    validate_amount(amount)?;
    let macros = left
        .macros
        .iter()
        .filter_map(|a| {
            right
                .macros
                .iter()
                .find(|b| a.id == b.id && a.target == b.target)
                .map(|b| (a.id.clone(), lerp(a.default, b.default, amount)))
        })
        .collect::<BTreeMap<_, _>>();

    // Interpolate the concrete scene fields so morphing remains useful even
    // when macro defaults happen to match.
    let mut left_project = project.clone();
    let mut right_project = project.clone();
    left.apply(&mut left_project, &PresetOverridesV1::default())?;
    right.apply(&mut right_project, &PresetOverridesV1::default())?;
    project.particle_system.count = lerp(
        left_project.particle_system.count as f32,
        right_project.particle_system.count as f32,
        amount,
    )
    .round() as u32;
    project.render_defaults.particle_size_pixels = lerp(
        left_project.render_defaults.particle_size_pixels,
        right_project.render_defaults.particle_size_pixels,
        amount,
    );
    project.camera.vertical_fov_degrees = lerp(
        left_project.camera.vertical_fov_degrees,
        right_project.camera.vertical_fov_degrees,
        amount,
    );
    project.camera.orbit_degrees_per_second = lerp(
        left_project.camera.orbit_degrees_per_second,
        right_project.camera.orbit_degrees_per_second,
        amount,
    );
    project.render_defaults.background = std::array::from_fn(|index| {
        lerp64(
            left_project.render_defaults.background[index],
            right_project.render_defaults.background[index],
            f64::from(amount),
        )
    });
    if amount < 0.5 {
        project.forces = left_project.forces;
        project.render_mode = left_project.render_mode;
        project.particle_system.emitter = left_project.particle_system.emitter;
    } else {
        project.forces = right_project.forces;
        project.render_mode = right_project.render_mode;
        project.particle_system.emitter = right_project.particle_system.emitter;
    }
    project.visual_preset = None;
    project.validate()?;
    Ok(PresetOverridesV1 { macros })
}

/// Configures the orbit camera to close exactly once over the project duration.
///
/// # Errors
///
/// Returns an error when the project duration is not positive and finite.
pub fn configure_seamless_camera_loop(project: &mut ProjectV1) -> Result<(), ProjectError> {
    if !project.duration_seconds.is_finite() || project.duration_seconds <= 0.0 {
        return Err(ProjectError::Validation(
            "a seamless loop requires a positive finite project duration".into(),
        ));
    }
    project.camera.mode = crate::CameraModeV1::Orbit;
    project.camera.orbit_degrees_per_second = 360.0 / project.duration_seconds;
    project.camera.dolly_units_per_second = 0.0;
    project.camera.drift_amplitude = [0.0; 3];
    project.camera.shake_amplitude = 0.0;
    Ok(())
}

fn validate_amount(amount: f32) -> Result<(), ProjectError> {
    if amount.is_finite() && (0.0..=1.0).contains(&amount) {
        Ok(())
    } else {
        Err(ProjectError::Validation(
            "creative-tool amount must be between 0 and 1".into(),
        ))
    }
}

const fn lerp(left: f32, right: f32, amount: f32) -> f32 {
    left + (right - left) * amount
}

const fn lerp64(left: f64, right: f64, amount: f64) -> f64 {
    left + (right - left) * amount
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn unit_hash(seed: u64, stream: u64) -> f32 {
    let mut value = seed ^ stream.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    (value as u32) as f32 / u32::MAX as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn path(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    #[test]
    fn randomization_is_deterministic_and_bounded() {
        let preset = VisualPresetV1::load(path("presets/star-system.json")).unwrap();
        let a = randomize_preset_macros(&preset, 42, 0.35).unwrap();
        let b = randomize_preset_macros(&preset, 42, 0.35).unwrap();
        assert_eq!(a, b);
        for parameter in &preset.macros {
            let value = a.macros[&parameter.id];
            assert!((parameter.minimum..=parameter.maximum).contains(&value));
        }
    }

    #[test]
    fn zero_mutation_returns_defaults() {
        let preset = VisualPresetV1::load(path("presets/nebula.json")).unwrap();
        let values = randomize_preset_macros(&preset, 9, 0.0).unwrap();
        for parameter in &preset.macros {
            assert!((values.macros[&parameter.id] - parameter.default).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn seamless_loop_closes_camera_orbit() {
        let mut project = ProjectV1::load(path("examples/star-orbit.rustique.json")).unwrap();
        configure_seamless_camera_loop(&mut project).unwrap();
        assert!(
            (project.camera.orbit_degrees_per_second * project.duration_seconds - 360.0).abs()
                < 0.001
        );
        assert!(
            project
                .camera
                .drift_amplitude
                .iter()
                .all(|value| value.abs() < f32::EPSILON)
        );
    }

    #[test]
    fn unrelated_presets_can_morph_at_endpoints() {
        let left = VisualPresetV1::load(path("presets/star-system.json")).unwrap();
        let right = VisualPresetV1::load(path("presets/nebula.json")).unwrap();
        let mut project = ProjectV1::load(path("examples/star-orbit.rustique.json")).unwrap();
        let overrides = morph_presets(&left, &right, 1.0, &mut project).unwrap();
        assert!(overrides.macros.is_empty());
        assert_eq!(project.render_mode, right.render_mode);
        assert!(
            (project.render_defaults.particle_size_pixels
                - right.render_defaults.particle_size_pixels)
                .abs()
                < f32::EPSILON
        );
    }
}
