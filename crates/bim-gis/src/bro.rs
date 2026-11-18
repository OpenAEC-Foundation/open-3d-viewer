//! The Dutch register of the subsurface (BRO): which cone penetration tests lie around a place,
//! and where to ask for one. Open data, no key and no account.
//!
//! A request tells the register which place is looked at; nothing else goes along. The caller
//! sends it: no network here. The test itself is read by `bim-civil`.

use crate::rd::{LatLon, Rd};

const TESTS: &str = "https://publiek.broservices.nl/sr/cpt/v1";
/// Where the soundings come from, for the properties of what is made of them.
pub const SOURCE: &str = "Basisregistratie Ondergrond (BRO), cone penetration tests";

/// The address a search for tests is posted to.
pub fn search_url() -> String {
    format!("{TESTS}/characteristics/searches?requestReference=open-bim-suite")
}

/// What is posted to search the tests within `radius` metres of a place (JSON).
pub fn search_body(center: LatLon, radius: f64) -> String {
    format!(r#"{{"area":{{"enclosingCircle":{{"center":{{"lat":{:.7},"lon":{:.7}}},"radius":{:.3}}}}}}}"#, center.lat, center.lon, radius / 1000.0)
}

/// The address of one test.
pub fn test_url(id: &str) -> String {
    format!("{TESTS}/objects/{id}")
}

/// A test the search found.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub id: String,
    pub place: Rd,
}

/// The tests in the answer to a search (XML), in the order of the answer. A test without a place
/// in the national grid, or taken out of the register, is left out.
pub fn parse_search(answer: &str) -> Result<Vec<Found>, String> {
    if !answer.contains("dispatchCharacteristicsResponse") {
        return Err("the register answered something else than a list of tests".into());
    }
    let between = |text: &str, open: &str, close: &str| {
        let start = text.find(open)? + open.len();
        Some(text[start..start + text[start..].find(close)?].to_string())
    };
    Ok(answer
        .split("<dispatchDocument>")
        .skip(1)
        .filter_map(|document| {
            let id = between(document, "<brocom:broId>", "<")?;
            if between(document, "<brocom:deregistered>", "<").is_some_and(|word| word.trim() == "ja") {
                return None;
            }
            let delivered = &document[document.find("deliveredLocation")?..];
            let pos = between(delivered, "<gml:pos>", "<")?;
            let mut numbers = pos.split_whitespace().map(str::parse::<f64>);
            let place = Rd::new(numbers.next()?.ok()?, numbers.next()?.ok()?);
            place.valid().then_some(Found { id: id.trim().to_string(), place })
        })
        .collect())
}

/// The `most` tests nearest to `center`, the nearest first.
pub fn nearest(mut found: Vec<Found>, center: Rd, most: usize) -> Vec<Found> {
    found.sort_by(|a, b| a.place.distance(center).total_cmp(&b.place.distance(center)));
    found.truncate(most);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"<dispatchCharacteristicsResponse><numberOfDocuments>3</numberOfDocuments><dispatchDocument><CPT_C gml:id="BRO_0002"><brocom:broId>CPT000000000785</brocom:broId><brocom:deregistered>nee</brocom:deregistered><brocom:standardizedLocation srsName="urn:ogc:def:crs:EPSG::4258"><gml:pos>52.156296290 5.382118350</gml:pos></brocom:standardizedLocation><brocom:deliveredLocation srsName="http://www.opengis.net/def/crs/EPSG/0/28992"><gml:pos>154652.00000000 463125.00000000</gml:pos></brocom:deliveredLocation></CPT_C></dispatchDocument><dispatchDocument><CPT_C><brocom:broId>CPT000000000001</brocom:broId><brocom:deregistered>ja</brocom:deregistered><brocom:deliveredLocation><gml:pos>154700.0 463100.0</gml:pos></brocom:deliveredLocation></CPT_C></dispatchDocument><dispatchDocument><CPT_C><brocom:broId>CPT000000000002</brocom:broId><brocom:deregistered>nee</brocom:deregistered><brocom:deliveredLocation><gml:pos>154800.0 463172.0</gml:pos></brocom:deliveredLocation></CPT_C></dispatchDocument></dispatchCharacteristicsResponse>"#;

    #[test]
    fn a_search_is_posted_as_a_circle_in_kilometres() {
        assert_eq!(search_body(LatLon::new(52.1567, 5.3842), 250.0), r#"{"area":{"enclosingCircle":{"center":{"lat":52.1567000,"lon":5.3842000},"radius":0.250}}}"#);
        assert!(search_url().starts_with("https://publiek.broservices.nl/sr/cpt/v1/characteristics/searches"));
        assert_eq!(test_url("CPT000000000785"), "https://publiek.broservices.nl/sr/cpt/v1/objects/CPT000000000785");
    }

    #[test]
    fn the_answer_lists_the_tests_with_their_places() {
        let found = parse_search(ANSWER).unwrap();
        // the test taken out of the register is left out
        assert_eq!(found.iter().map(|test| test.id.as_str()).collect::<Vec<_>>(), ["CPT000000000785", "CPT000000000002"]);
        // the place in the national grid, not the latitude and longitude before it
        assert_eq!(found[0].place, Rd::new(154_652.0, 463_125.0));
        assert!(parse_search("<html>busy</html>").is_err());
    }

    #[test]
    fn the_nearest_tests_come_first() {
        let found = parse_search(ANSWER).unwrap();
        let near = nearest(found, Rd::new(154_792.0, 463_172.0), 1);
        assert_eq!(near.len(), 1);
        assert_eq!(near[0].id, "CPT000000000002");
    }
}
