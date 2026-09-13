# Experimental creative tools

The desktop editor's **Creative tools** panel provides deterministic preset mutation,
preset morphing, and one-click camera loops. These operations are implemented in
`project-format`, not React, so saved projects render identically in headless mode.

Mutation uses the project's stored seed plus a variation number. The amount slider
ranges from 0 (preset defaults) to 1 (the complete declared macro range). Values never
leave the preset's bounds.

Morphing interpolates particle count, particle size, camera FOV and orbit speed, and
background color. Discrete emitter, force, and render-mode state switches at the
midpoint. Applying a morph resolves the result into project data.

Camera loop generation sets orbit speed to `360 / duration_seconds` and disables dolly,
drift, and shake. The simulation itself remains deterministic; effects with non-periodic
state can still make the first and final rendered frames differ.
