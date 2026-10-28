//! The entities beyond mobs and items that a server tracks to its clients
//! (`ChunkMap.TrackedEntity`, `ServerEntity`): boats, minecarts, armor
//! stands, item frames, paintings and the entities Horizon keeps without
//! simulating them, each as vanilla pairs it (`getAddEntityPacket` and the
//! synched data that differs from the defaults), and who rides what.
use super::Game;




impl Game<'_> {}
impl crate::game::Sim<'_> {
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armor_stand_poses_send_only_what_differs() {
        let mut stand = horizon_client::entities::ArmorStand::new(1, Default::default(), 0.0);
        stand.pose[0] = [10.0, 20.0, 0.0];
        let meta = stand_poses(&stand);
        assert_eq!(
            meta,
            vec![(
                16,
                serializer::ROTATIONS,
                Value::Floats(vec![10.0, 20.0, 0.0])
            )]
        );
        let plain = horizon_client::entities::ArmorStand::new(1, Default::default(), 0.0);
        assert!(stand_poses(&plain).is_empty());
    }
}
