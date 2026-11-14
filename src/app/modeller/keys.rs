//! The Modeller's keys, as the reference modeller has them: a letter starts a tool at once, and
//! two letters typed within [`WINDOW`] are one command (MV moves, CL places a column). A letter
//! that starts such a pair waits for the second one; when none comes in time it does what it
//! does alone (C makes a section, D a door).
//!
//! Only plain letters count: with Ctrl or Alt a key is a shortcut of the app, and a key a text
//! field takes never gets here. PageUp and PageDown step through the levels (the shell maps them).

use web_time::{Duration, Instant};

/// How long a letter that starts a pair waits for the second one.
pub const WINDOW: Duration = Duration::from_millis(750);

/// A command of the Modeller a key gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelKey {
    Select,
    Wall,
    Slab,
    Column,
    Beam,
    System,
    Object,
    Door,
    Window,
    Grid,
    Section,
    Dimension,
    /// G: grabs the picked points in edit mode, else moves the selection.
    Grab,
    Move,
    Copy,
    Rotate,
    Mirror,
    Delete,
    LevelUp,
    LevelDown,
}

/// The letters that are a command alone (lower case).
pub const SINGLE: &[(char, ModelKey)] = &[
    ('s', ModelKey::Select),
    ('w', ModelKey::Wall),
    ('g', ModelKey::Grab),
    ('i', ModelKey::Grid),
    ('d', ModelKey::Door),
    ('o', ModelKey::Window),
    ('c', ModelKey::Section),
];

/// The two letters that are a command together (lower case).
pub const PAIRS: &[(&str, ModelKey)] = &[
    ("mv", ModelKey::Move),
    ("co", ModelKey::Copy),
    ("ro", ModelKey::Rotate),
    ("mm", ModelKey::Mirror),
    ("cl", ModelKey::Column),
    ("bm", ModelKey::Beam),
    ("di", ModelKey::Dimension),
    ("fl", ModelKey::Slab),
    ("cw", ModelKey::System),
    ("ob", ModelKey::Object),
];

/// The letter of a command alone.
fn single(c: char) -> Option<ModelKey> {
    SINGLE.iter().find(|(k, _)| *k == c).map(|(_, m)| *m)
}

/// Whether a pair starts with this letter.
fn starts_pair(c: char) -> bool {
    PAIRS.iter().any(|(p, _)| p.starts_with(c))
}

/// What a letter typed comes to: a command of the Modeller, or a letter it has no use for, given
/// back to the app's own shortcuts (F fits, H hides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Key(ModelKey),
    Letter(char),
}

/// A letter alone: its command, else the letter itself.
fn alone(c: char) -> Step {
    single(c).map_or(Step::Letter(c), Step::Key)
}

/// The letters typed so far: the one waiting for its pair, and when it was typed.
#[derive(Debug, Clone, Default)]
pub struct KeySequence {
    pending: Option<(char, Instant)>,
}

impl KeySequence {
    /// Letter `c` typed at `now`: what it comes to, in order (nothing while it waits for a
    /// second letter).
    pub fn steps(&mut self, c: char, now: Instant) -> Vec<Step> {
        let c = c.to_ascii_lowercase();
        let mut out = Vec::new();
        if let Some((first, at)) = self.pending.take() {
            if now.duration_since(at) <= WINDOW {
                let pair: String = [first, c].iter().collect();
                if let Some((_, m)) = PAIRS.iter().find(|(p, _)| *p == pair) {
                    return vec![Step::Key(*m)];
                }
            }
            // no such pair, or too late: the first is what it is alone
            out.push(alone(first));
        }
        if starts_pair(c) {
            self.pending = Some((c, now));
        } else {
            out.push(alone(c));
        }
        out
    }

    /// The commands of [`KeySequence::steps`] alone.
    pub fn press(&mut self, c: char, now: Instant) -> Vec<ModelKey> {
        self.steps(c, now).into_iter().filter_map(|s| match s {
            Step::Key(k) => Some(k),
            Step::Letter(_) => None,
        }).collect()
    }

    /// The time is `now`: a letter that waited longer than [`WINDOW`] is what it is alone.
    pub fn expire_step(&mut self, now: Instant) -> Option<Step> {
        let (c, at) = self.pending?;
        if now.duration_since(at) <= WINDOW {
            return None;
        }
        self.pending = None;
        Some(alone(c))
    }

    /// The command of [`KeySequence::expire_step`], if it is one.
    pub fn expire(&mut self, now: Instant) -> Option<ModelKey> {
        match self.expire_step(now)? {
            Step::Key(k) => Some(k),
            Step::Letter(_) => None,
        }
    }

    /// The letter waiting for its pair.
    pub fn pending(&self) -> Option<char> {
        self.pending.map(|(c, _)| c)
    }

    /// Forgets the letter waiting.
    pub fn clear(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use web_time::Duration;

    #[test]
    fn single_letters_run_at_once_unless_they_start_a_pair() {
        let t = Instant::now();
        let mut k = KeySequence::default();
        assert_eq!(k.press('w', t), vec![ModelKey::Wall]);
        assert_eq!(k.press('i', t), vec![ModelKey::Grid]);
        // c starts co and cl: it waits
        assert!(k.press('c', t).is_empty());
        assert_eq!(k.pending(), Some('c'));
        assert_eq!(k.expire(t + Duration::from_millis(500)), None);
        assert_eq!(k.expire(t + Duration::from_millis(760)), Some(ModelKey::Section));
        assert_eq!(k.pending(), None);
    }

    #[test]
    fn two_letters_within_the_window_are_one_command() {
        let t = Instant::now();
        let mut k = KeySequence::default();
        assert!(k.press('m', t).is_empty());
        assert_eq!(k.press('v', t + Duration::from_millis(300)), vec![ModelKey::Move]);
        assert!(k.press('c', t).is_empty());
        assert_eq!(k.press('l', t + Duration::from_millis(100)), vec![ModelKey::Column]);
        // too late: the first runs alone, the second as itself
        assert!(k.press('d', t).is_empty());
        assert_eq!(k.press('w', t + Duration::from_millis(800)), vec![ModelKey::Door, ModelKey::Wall]);
        // a pending letter with no command of its own and no pair: nothing
        assert!(k.press('m', t).is_empty());
        assert_eq!(k.press('x', t + Duration::from_millis(100)), vec![]);
        assert_eq!(k.pending(), None);
        // c, then a letter that makes no pair with it: the section, then the grid
        assert!(k.press('c', t).is_empty());
        assert_eq!(k.press('i', t + Duration::from_millis(100)), vec![ModelKey::Section, ModelKey::Grid]);
        // upper case is the same key
        assert_eq!(k.press('W', t), vec![ModelKey::Wall]);
    }

    #[test]
    fn letters_without_a_command_of_the_modeller_are_given_back() {
        let t = Instant::now();
        let mut k = KeySequence::default();
        // h hides in the app: not the Modeller's, so it is given back at once
        assert_eq!(k.steps('h', t), vec![Step::Letter('h')]);
        // f starts fl: it waits, and when no l comes it is given back
        assert!(k.steps('f', t).is_empty());
        assert_eq!(k.expire_step(t + Duration::from_millis(800)), Some(Step::Letter('f')));
        // m then x within the window: no pair, both given back
        assert!(k.steps('m', t).is_empty());
        assert_eq!(k.steps('x', t + Duration::from_millis(100)), vec![Step::Letter('m'), Step::Letter('x')]);
        // a command stays a command
        assert_eq!(k.steps('w', t), vec![Step::Key(ModelKey::Wall)]);
    }

    #[test]
    fn every_pair_and_letter_is_in_the_table_once() {
        let mut seen = std::collections::HashSet::new();
        for (c, _) in SINGLE {
            assert!(seen.insert(c.to_string()));
        }
        for (p, _) in PAIRS {
            assert!(seen.insert(p.to_string()) && p.len() == 2, "{p}");
        }
    }
}
