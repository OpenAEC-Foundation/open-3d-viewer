//! Joins of linear members: where members meet, a join trims their ends, takes material out of
//! them, adds material to them, and may have parts of its own (plates, bolts, welds). A skirting
//! board's mitre and a steel end plate are the same kind of thing; they differ in their parameters
//! and in the shapes they cut and add.
//!
//! [`cuts`] is the one function that works out what a join does. Join elements (a connection in
//! the model) and the joints a system generates call it alike, so both look and behave the same.
//! [`finish`] then makes a member's body: its profile swept between its trims, with its additions
//! united and its removals taken out.

use crate::boolean::{boolean, BooleanOp};
use crate::element::{linear_axes, LinearDef, PartSolid};
use crate::model::JOIN_TOLERANCE;
use crate::profile::Profile;
use crate::solid::{Axes, Plane, Solid};
use crate::Refusal;
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

/// Which end of a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberEnd {
    Start,
    End,
}

/// A member taking part in a join, in the join's frame: its frame (`z` along it from its start),
/// its length and its profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub axes: Axes,
    pub length: f64,
    pub profile: Profile,
}

impl Member {
    /// The member a linear element's definition makes.
    pub fn of(l: &LinearDef) -> Result<Self, Refusal> {
        let (axes, length) = linear_axes(l)?;
        Ok(Self { axes, length, profile: l.profile.profile()? })
    }

    pub fn start(&self) -> DVec3 {
        self.axes.origin
    }

    pub fn end(&self) -> DVec3 {
        self.axes.origin + self.axes.z * self.length
    }

    /// An end and the direction from it into the member.
    fn end_of(&self, which: MemberEnd) -> (DVec3, DVec3) {
        match which {
            MemberEnd::Start => (self.start(), self.axes.z),
            MemberEnd::End => (self.end(), -self.axes.z),
        }
    }

    /// The plane across its end `which`.
    pub fn square_end(&self, which: MemberEnd) -> Plane {
        match which {
            MemberEnd::Start => Plane { point: self.start(), normal: -self.axes.z },
            MemberEnd::End => Plane { point: self.end(), normal: self.axes.z },
        }
    }

    /// The farthest its profile reaches from its axis.
    fn radius(&self) -> f64 {
        self.profile.outer().iter().map(|p| p.length()).fold(0.0, f64::max)
    }
}

/// The kinds of join.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinKind {
    /// Both members are cut by the plane that halves the angle between them.
    Mitre,
    /// The second member stops at the face of the first, which runs through.
    Butt,
}

/// What a join does to its members (indices into the members given to [`cuts`]), in the join's
/// frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JoinResult {
    /// The plane a member's end is cut to.
    pub trims: Vec<(usize, MemberEnd, Plane)>,
    /// Solids taken out of a member (holes, a notch).
    pub removals: Vec<(usize, Solid)>,
    /// Solids added to a member (a tenon, a tooth), so its own quantities stay right.
    pub additions: Vec<(usize, Solid)>,
    /// The join's own parts (plates, bolts, welds).
    pub parts: Vec<PartSolid>,
}

/// What a join of `kind` does to `members`, all in one frame. A mitre and a butt take two
/// members; in a butt the first runs through.
pub fn cuts(kind: JoinKind, members: &[Member]) -> Result<JoinResult, Refusal> {
    let [first, second] = members else { return Err(Refusal::TwoMembers) };
    let mut result = JoinResult::default();
    match kind {
        JoinKind::Butt => {
            let (end, plane) = butt(first, second)?;
            result.trims.push((1, end, plane));
        }
        JoinKind::Mitre => {
            let ((e0, e1), plane) = mitre(first, second)?;
            result.trims.push((0, e0, Plane { point: plane.point, normal: -plane.normal }));
            result.trims.push((1, e1, plane));
        }
    }
    Ok(result)
}

/// A member's body as its joins leave it: its profile swept between its trims (its own square
/// ends where none), its additions united with it and its removals taken out.
pub fn finish(member: &Member, start: Option<Plane>, end: Option<Plane>, additions: &[Solid], removals: &[Solid]) -> Result<Solid, Refusal> {
    let mut body = Solid::sweep_between(
        &member.profile,
        &member.axes,
        start.unwrap_or(member.square_end(MemberEnd::Start)),
        end.unwrap_or(member.square_end(MemberEnd::End)),
    )?;
    for a in additions {
        body = boolean(&body, a, BooleanOp::Union)?;
    }
    for r in removals {
        body = boolean(&body, r, BooleanOp::Difference)?;
    }
    Ok(body)
}

/// The distance from `q` to the segment `a`–`b`.
fn to_segment(q: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((q - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t - q).length()
}

/// A butt joint: the end of `secondary` nearest `primary` stops at the primary's face, the plane
/// along the primary at the reach of its profile towards the secondary.
fn butt(primary: &Member, secondary: &Member) -> Result<(MemberEnd, Plane), Refusal> {
    let axes = &primary.axes;
    let (a, b) = (primary.start(), primary.end());
    let which = if to_segment(secondary.start(), a, b) <= to_segment(secondary.end(), a, b) { MemberEnd::Start } else { MemberEnd::End };
    let (q, into) = secondary.end_of(which);
    if into.cross(axes.z).length() < 1e-6 {
        return Err(Refusal::Parallel);
    }
    // the direction from the primary's axis towards the secondary, square to the primary
    let n = (into - axes.z * into.dot(axes.z)).try_normalize().ok_or(Refusal::Parallel)?;
    let reach = primary.profile.support(DVec2::new(n.dot(axes.x), n.dot(axes.y)));
    let radius = primary.radius();
    if to_segment(q, a - axes.z * radius, b + axes.z * radius) > radius + JOIN_TOLERANCE {
        return Err(Refusal::NotTouching);
    }
    // the face: through the point of the axis nearest q, moved out by the reach
    let foot = a + axes.z * (q - a).dot(axes.z);
    Ok((which, Plane { point: foot + n * reach, normal: -n }))
}

/// A mitre: the nearest ends of the two members meet (within [`JOIN_TOLERANCE`]); both are cut by
/// the plane through their meeting point that halves the angle between them. The plane's normal
/// points out of the second member.
fn mitre(first: &Member, second: &Member) -> Result<((MemberEnd, MemberEnd), Plane), Refusal> {
    let ends = [MemberEnd::Start, MemberEnd::End];
    let (e0, e1) = ends
        .iter()
        .flat_map(|&p| ends.iter().map(move |&s| (p, s)))
        .min_by(|&(p1, s1), &(p2, s2)| {
            let d = |p, s| (first.end_of(p).0 - second.end_of(s).0).length();
            d(p1, s1).total_cmp(&d(p2, s2))
        })
        .unwrap();
    let ((q0, into0), (q1, into1)) = (first.end_of(e0), second.end_of(e1));
    if (q0 - q1).length() > JOIN_TOLERANCE {
        return Err(Refusal::NotTouching);
    }
    let n = (into0 - into1).try_normalize().ok_or(Refusal::Parallel)?;
    Ok(((e0, e1), Plane { point: (q0 + q1) / 2.0, normal: n }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileDef;

    fn member(start: DVec3, end: DVec3) -> Member {
        Member::of(&LinearDef { start, end, profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0 }).unwrap()
    }

    #[test]
    fn a_mitre_trims_both_and_a_butt_the_second() {
        let (a, b) = (member(DVec3::ZERO, DVec3::new(4.0, 0.0, 0.0)), member(DVec3::new(4.0, 0.0, 0.0), DVec3::new(4.0, 3.0, 0.0)));
        let mitre = cuts(JoinKind::Mitre, &[a.clone(), b.clone()]).unwrap();
        assert_eq!(mitre.trims.iter().map(|t| (t.0, t.1)).collect::<Vec<_>>(), vec![(0, MemberEnd::End), (1, MemberEnd::Start)]);
        let butt = cuts(JoinKind::Butt, &[a, b]).unwrap();
        assert_eq!(butt.trims.len(), 1);
        assert_eq!((butt.trims[0].0, butt.trims[0].1), (1, MemberEnd::Start));
        assert!(mitre.removals.is_empty() && mitre.additions.is_empty() && mitre.parts.is_empty());
    }

    #[test]
    fn a_join_takes_two_members() {
        let a = member(DVec3::ZERO, DVec3::X);
        assert_eq!(cuts(JoinKind::Mitre, std::slice::from_ref(&a)), Err(Refusal::TwoMembers));
        assert_eq!(cuts(JoinKind::Butt, &[a.clone(), a.clone(), a]), Err(Refusal::TwoMembers));
    }

    #[test]
    fn a_body_takes_its_additions_and_loses_its_removals() {
        // a beam 4 m long, 0.2 × 0.4, with a tenon on its end and a hole through it
        let m = member(DVec3::ZERO, DVec3::new(4.0, 0.0, 0.0));
        let block = |lo: DVec3, hi: DVec3| {
            let size = hi - lo;
            let axes = Axes::new(DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, lo.z), DVec3::X, DVec3::Z).unwrap();
            Solid::extrude(&Profile::rectangle(size.x, size.y).unwrap(), &axes, size.z).unwrap()
        };
        let tenon = block(DVec3::new(4.0, -0.05, -0.1), DVec3::new(4.1, 0.05, 0.1));
        let hole = block(DVec3::new(1.0, -0.2, -0.05), DVec3::new(1.1, 0.2, 0.05));
        let body = finish(&m, None, None, &[tenon], &[hole]).unwrap();
        let expected = 4.0 * 0.2 * 0.4 + 0.1 * 0.1 * 0.2 - 0.1 * 0.2 * 0.1;
        assert!((body.volume() - expected).abs() < 1e-9, "{} vs {expected}", body.volume());
    }
}
