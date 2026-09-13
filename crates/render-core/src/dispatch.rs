pub(crate) const COMPUTE_WORKGROUP_SIZE: u32 = 256;
const MAX_WORKGROUPS_PER_DIMENSION: u32 = 65_535;

/// Tiles a one-dimensional item count across the X/Y dispatch dimensions.
pub(crate) fn dispatch_dimensions(item_count: u32) -> (u32, u32) {
    let groups = item_count.div_ceil(COMPUTE_WORKGROUP_SIZE);
    let groups_x = groups.min(MAX_WORKGROUPS_PER_DIMENSION);
    (groups_x, groups.div_ceil(groups_x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_stays_on_x_at_the_portable_limit() {
        assert_eq!(
            dispatch_dimensions(MAX_WORKGROUPS_PER_DIMENSION * COMPUTE_WORKGROUP_SIZE),
            (MAX_WORKGROUPS_PER_DIMENSION, 1)
        );
    }

    #[test]
    fn dispatch_spills_large_counts_into_y() {
        assert_eq!(dispatch_dimensions(20_000_000), (65_535, 2));
    }
}
