//! The Modeller's keys, as the reference modeller has them: two letters typed within [`WINDOW`]
//! are one command (WA draws a wall, DR places a door, MV moves, CS makes another like the one
//! selected), and some letters are a command alone. A letter that starts a pair waits for the
//! second one; when none comes in time it does what it does alone (W a wall, D a door). G and S
//! do not wait: G grabs and S selects at once, and still start their pairs (GR, SB, SE).
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
    /// A text on the drawing shown.
    Text,
    /// Lines, a rectangle or a circle drawn on the view shown.
    DrawLine,
    DrawRectangle,
    DrawCircle,
    /// Another element like the one selected: its tool, with its type and sizes.
    CreateSimilar,
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
    ("wa", ModelKey::Wall),
    ("dr", ModelKey::Door),
    ("wn", ModelKey::Window),
    ("sb", ModelKey::Slab),
    ("gr", ModelKey::Grid),
    ("gl", ModelKey::Grid),
    ("sl", ModelKey::Slab),
    ("se", ModelKey::Section),
    ("tx", ModelKey::Text),
    ("li", ModelKey::DrawLine),
    ("rc", ModelKey::DrawRectangle),
    ("ci", ModelKey::DrawCircle),
    ("cs", ModelKey::CreateSimilar),
    ("de", ModelKey::Delete),
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

/// The letters that start a pair and still run at once: their command alone does no harm before
/// the pair's (G grabs, and GR lets go again for the grid tool).
pub const EAGER: &[char] = &['g', 's'];

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
    /// The letter, when it was typed, and whether it ran already (see [`EAGER`]).
    pending: Option<(char, Instant, bool)>,
}

impl KeySequence {
    /// Letter `c` typed at `now`: what it comes to, in order (nothing while it waits for a
    /// second letter).
    pub fn steps(&mut self, c: char, now: Instant) -> Vec<Step> {
        let c = c.to_ascii_lowercase();
        let mut out = Vec::new();
        if let Some((first, at, ran)) = self.pending.take() {
            if now.duration_since(at) <= WINDOW {
                let pair: String = [first, c].iter().collect();
                if let Some((_, m)) = PAIRS.iter().find(|(p, _)| *p == pair) {
                    return vec![Step::Key(*m)];
                }
            }
            // no such pair, or too late: the first is what it is alone
            if !ran {
                out.push(alone(first));
            }
        }
        if starts_pair(c) {
            let eager = EAGER.contains(&c);
            if eager {
                out.push(alone(c));
            }
            self.pending = Some((c, now, eager));
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
        let (c, at, ran) = self.pending?;
        if now.duration_since(at) <= WINDOW {
            return None;
        }
        self.pending = None;
        (!ran).then(|| alone(c))
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
        self.pending.map(|(c, ..)| c)
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
    fn a_letter_that_starts_no_pair_runs_at_once_and_one_that_does_waits() {
        let t = Instant::now();
        let mut k = KeySequence::default();
        assert_eq!(k.press('i', t), vec![ModelKey::Grid]);
        // w starts wa and wn: it waits, and alone it is the wall
        assert!(k.press('w', t).is_empty());
        assert_eq!(k.pending(), Some('w'));
        assert_eq!(k.expire(t + Duration::from_millis(500)), None);
        assert_eq!(k.expire(t + Duration::from_millis(760)), Some(ModelKey::Wall));
        assert_eq!(k.pending(), None);
    }

    #[test]
    fn two_letters_within_the_window_are_one_command() {
        let t = Instant::now();
        let mut k = KeySequence::default();
        let mut pair = |a: char, b: char| {
            assert!(k.press(a, t).is_empty(), "{a}");
            k.press(b, t + Duration::from_millis(300))
        };
        for (a, b, key) in [
            ('w', 'a', ModelKey::Wall),
            ('d', 'r', ModelKey::Door),
            ('w', 'n', ModelKey::Window),
            ('c', 'l', ModelKey::Column),
            ('b', 'm', ModelKey::Beam),
            ('d', 'i', ModelKey::Dimension),
            ('d', 'e', ModelKey::Delete),
            ('m', 'v', ModelKey::Move),
            ('c', 'o', ModelKey::Copy),
            ('r', 'o', ModelKey::Rotate),
            ('m', 'm', ModelKey::Mirror),
            ('c', 's', ModelKey::CreateSimilar),
            ('t', 'x', ModelKey::Text),
        ] {
            assert_eq!(pair(a, b), vec![key], "{a}{b}");
        }
        // too late: the first runs alone, the second waits for its own pair
        assert!(k.press('d', t).is_empty());
        assert_eq!(k.press('w', t + Duration::from_millis(800)), vec![ModelKey::Door]);
        k.clear();
        // a pending letter with no command of its own and no pair: nothing
        assert!(k.press('m', t).is_empty());
        assert_eq!(k.press('x', t + Duration::from_millis(100)), vec![]);
        assert_eq!(k.pending(), None);
        // upper case is the same key
        assert_eq!(k.press('I', t), vec![ModelKey::Grid]);
    }

    #[test]
    fn g_and_s_run_at_once_and_still_start_their_pairs() {
        let t = Instant::now();
        let mut k = KeySequence::default();
        // G grabs at once; R right after it makes it the grid tool instead
        assert_eq!(k.press('g', t), vec![ModelKey::Grab]);
        assert_eq!(k.press('r', t + Duration::from_millis(200)), vec![ModelKey::Grid]);
        // alone it ran already: nothing more when its time is up, nor before another letter
        assert_eq!(k.press('g', t), vec![ModelKey::Grab]);
        assert_eq!(k.expire(t + Duration::from_millis(800)), None);
        assert_eq!(k.pending(), None);
        assert_eq!(k.press('g', t), vec![ModelKey::Grab]);
        assert_eq!(k.press('i', t + Duration::from_millis(100)), vec![ModelKey::Grid]);
        // S selects at once, and SB is the slab, SE the section
        assert_eq!(k.press('s', t), vec![ModelKey::Select]);
        assert_eq!(k.press('b', t + Duration::from_millis(100)), vec![ModelKey::Slab]);
        assert_eq!(k.press('s', t), vec![ModelKey::Select]);
        assert_eq!(k.press('e', t + Duration::from_millis(100)), vec![ModelKey::Section]);
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
        assert_eq!(k.steps('i', t), vec![Step::Key(ModelKey::Grid)]);
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
        // a letter that runs at once has a command alone and starts a pair
        for c in EAGER {
            assert!(single(*c).is_some() && starts_pair(*c), "{c}");
        }
    }
}
