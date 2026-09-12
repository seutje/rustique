#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerspectiveCamera {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub vertical_fov_degrees: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

impl PerspectiveCamera {
    #[must_use]
    pub fn view_projection(self, aspect_ratio: f32) -> [[f32; 4]; 4] {
        multiply(
            perspective(
                self.vertical_fov_degrees.to_radians(),
                aspect_ratio,
                self.near_plane,
                self.far_plane,
            ),
            look_at(self.position, self.target, self.up),
        )
    }
}

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let forward = normalize(subtract(target, eye));
    let right = normalize(cross(forward, up));
    let camera_up = cross(right, forward);
    [
        [right[0], camera_up[0], -forward[0], 0.0],
        [right[1], camera_up[1], -forward[1], 0.0],
        [right[2], camera_up[2], -forward[2], 0.0],
        [
            -dot(right, eye),
            -dot(camera_up, eye),
            dot(forward, eye),
            1.0,
        ],
    ]
}

fn perspective(fov: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let focal = 1.0 / (fov * 0.5).tan();
    [
        [focal / aspect, 0.0, 0.0, 0.0],
        [0.0, focal, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ]
}

fn multiply(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0; 4]; 4];
    for column in 0..4 {
        for row in 0..4 {
            result[column][row] = (0..4).map(|index| a[index][row] * b[column][index]).sum();
        }
    }
    result
}

fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = dot(value, value).sqrt();
    if length > f32::EPSILON {
        [value[0] / length, value[1] / length, value[2] / length]
    } else {
        [0.0, 0.0, -1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_changes_with_aspect_but_not_camera_position() {
        let camera = PerspectiveCamera {
            position: [0.0, 0.0, 3.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            vertical_fov_degrees: 45.0,
            near_plane: 0.01,
            far_plane: 1000.0,
        };
        let wide = camera.view_projection(16.0 / 9.0);
        let square = camera.view_projection(1.0);
        assert!((wide[0][0] - square[0][0]).abs() > f32::EPSILON);
        assert!((wide[3][2] - square[3][2]).abs() < f32::EPSILON);
    }
}
