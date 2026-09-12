# Camera system

Projects store a perspective camera with position, look-at target, up vector,
vertical field of view, and near/far planes. `static` preserves the base pose;
`orbit` rotates its position around the look-at target at a stored angular rate.

The camera also supports a linear dolly rate, deterministic sinusoidal drift,
audio-reactive FOV, and audio-reactive shake. Add modulation mappings targeting
`camera_fov` or `camera_shake`; the camera's `fov_modulation_degrees` and
`shake_amplitude` fields scale those normalized signals.

All procedural movement is evaluated from `frame_index / fps` and the project
seed. Resolution only contributes the projection aspect ratio and never changes
the camera's world-space pose.
