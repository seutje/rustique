# Deterministic Simulation

Offline simulation advances only from a frame index, project FPS, and fixed
substep count. For frame `n`, scene time is `n / project_fps`; preview refresh
rate and render-target dimensions do not enter the compute update.

Particle initialization uses stable integer hashing of the stored seed and
particle index. GPU respawning uses the seed, particle index, and timeline frame,
so it does not rely on platform random-number generators or wall-clock state.
Backward seeking resets both ping-pong buffers and replays fixed steps from frame
zero. This early strategy favors correctness over seek speed.

## Practical limits

Integer initialization and scheduling are reproducible. Floating-point shader
results can still differ slightly across GPU vendors, drivers, compiler versions,
and backends because operation contraction and precision behavior are not fully
portable. Exact byte equality is expected on a fixed engine, GPU, driver, and
backend; cross-device results are expected to be visually equivalent rather than
bit-identical.
