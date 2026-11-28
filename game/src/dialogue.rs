//! People in the street the player can talk to (the Web `dlg(title, text, buttons)`
//! dialogs used by the ROC remnants, the JSDF liaison and the families upstairs).
//!
//! A system that owns a person publishes where they stand in [`NpcSpots`] every
//! frame. Interact picks the nearest reachable spot ([`crate::worldplay`]) and
//! writes [`TalkTo`]; the owner answers by putting a [`DialogueCard`] in
//! [`Dialogue`]. The panel UI shows it and returns the chosen option as
//! [`DialogueChosen`]. The world keeps running while a card is open, exactly as
//! the Web dialogs do, so an owner must re-check its state when an option arrives.

use bevy::prelude::*;

/// Who the player is talking to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NpcKey {
    /// Squad index of a v24.3 ROC remnant position.
    Remnant(u8),
    /// v24.4 JSDF liaison officer.
    Liaison,
    /// v24.8 the family waiting upstairs.
    Family,
    /// The player's own JSDF field radio (used from the bag).
    Radio,
    /// v23.4 a stairwell: climb one floor.
    StairsUp,
    /// v23.4 a stairwell: go down one floor.
    StairsDown,
    /// v22.5 the door of a sniper's building: climb up and overpower him.
    Sniper(u8),
}

/// What an option does; the owning system interprets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogueKey {
    RemnantSupply(u8),
    RemnantBandage(u8),
    RemnantIntel(u8),
    LiaisonBattery,
    CallAirdrop,
    CallStrike,
    CallApache,
    FamilyGo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DialogueOption {
    /// Short button label.
    pub label: String,
    /// The row text beside the button.
    pub detail: String,
    pub key: DialogueKey,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct DialogueCard {
    pub title: String,
    pub body: String,
    pub options: Vec<DialogueOption>,
}

/// The card on screen, if any.
#[derive(Resource, Default, Debug, Clone)]
pub struct Dialogue(pub Option<DialogueCard>);

/// A person the player can address from here.
#[derive(Clone, Debug, PartialEq)]
pub struct NpcSpot {
    pub key: NpcKey,
    pub pos: Vec2,
    /// Distance within which Interact reaches them.
    pub reach: f32,
    pub label: String,
    pub detail: String,
    /// Reachable without a clear line through the street-level walls and
    /// furniture (stairs, and anything on an upper floor).
    pub through: bool,
}

/// Every addressable person this frame, written by their owners.
#[derive(Resource, Default, Debug, Clone)]
pub struct NpcSpots(pub Vec<NpcSpot>);

impl NpcSpots {
    /// Replace the spots owned by `owns` with `spots`.
    pub fn publish(
        &mut self,
        owns: impl Fn(NpcKey) -> bool,
        spots: impl IntoIterator<Item = NpcSpot>,
    ) {
        self.0.retain(|s| !owns(s.key));
        self.0.extend(spots);
    }

    /// The nearest person within reach of `pos` that `visible` accepts.
    pub fn nearest(&self, pos: Vec2, visible: impl Fn(Vec2) -> bool) -> Option<&NpcSpot> {
        self.0
            .iter()
            .filter(|s| s.pos.distance(pos) < s.reach && (s.through || visible(s.pos)))
            .min_by(|a, b| {
                a.pos
                    .distance_squared(pos)
                    .total_cmp(&b.pos.distance_squared(pos))
            })
    }
}

/// Interact reached this person.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TalkTo(pub NpcKey);

/// The player pressed an option on the open card.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogueChosen(pub DialogueKey);

pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Dialogue>()
            .init_resource::<NpcSpots>()
            .add_message::<TalkTo>()
            .add_message::<DialogueChosen>()
            .add_systems(
                OnExit(crate::game_flow::GameFlow::Playing),
                |mut d: ResMut<Dialogue>| d.0 = None,
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(key: NpcKey, x: f32) -> NpcSpot {
        NpcSpot {
            key,
            pos: Vec2::new(x, 0.0),
            reach: 3.2,
            label: String::new(),
            detail: String::new(),
            through: false,
        }
    }

    #[test]
    fn publish_replaces_only_the_owners_spots() {
        let mut spots = NpcSpots::default();
        spots.publish(|_| false, [spot(NpcKey::Liaison, 0.0)]);
        spots.publish(
            |k| matches!(k, NpcKey::Remnant(_)),
            [spot(NpcKey::Remnant(0), 1.0), spot(NpcKey::Remnant(1), 2.0)],
        );
        spots.publish(
            |k| matches!(k, NpcKey::Remnant(_)),
            [spot(NpcKey::Remnant(2), 1.0)],
        );
        let keys: Vec<_> = spots.0.iter().map(|s| s.key).collect();
        assert_eq!(keys, vec![NpcKey::Liaison, NpcKey::Remnant(2)]);
    }

    #[test]
    fn nearest_respects_reach_and_sight() {
        let mut spots = NpcSpots::default();
        spots.publish(
            |_| false,
            [spot(NpcKey::Remnant(0), 2.0), spot(NpcKey::Liaison, 1.0)],
        );
        let at = Vec2::ZERO;
        assert_eq!(spots.nearest(at, |_| true).unwrap().key, NpcKey::Liaison);
        assert_eq!(
            spots.nearest(at, |p| p.x > 1.5).unwrap().key,
            NpcKey::Remnant(0)
        );
        assert!(spots.nearest(Vec2::new(10.0, 0.0), |_| true).is_none());
    }
}
