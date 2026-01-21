use std::cmp;
use std::fmt::{self, Display};
use std::ops::Deref;

/// A sequence of alternating digit and non-digit string parts.
///
/// For example, the version sequence for the string `v15.010-rc.1` consists of:
///
///   - The non-digit part `v`
///   - The digit part `15`, with numeric value 15
///   - The non-digit part `.`
///   - The digit part `010`, with numeric value 10
///   - The non-digit part `-rc.`
///   - The digit part `1`, with numeric value 1
///
/// # Ordering and Equality
///
/// `Version` values are compared lexicographically as described in the [`Ord`] documentation
/// on a part-by-part basis, with the following additional properties:
///
///   - Digit parts are lexicographically less than non-digit parts regardless of value.
///   - Digit parts are compared by numeric value rather than textual representation.
///
/// For example, the following comparisons hold:
///
///   - `v1.99.99 < v2.0.0`
///   - `v1.050 == v1.50` (inconsistent with textual comparison, but semantically correct)
///   - `2.0.0 < v1.0.0` (inconsistent with semantic meaning)
///   - `v1.00 < v1.0-beta.1` (inconsistent with textual ordering and semantic meaning)
///
/// As the examples attempt to demonstrate, this ordering works best to compare version sequences
/// that follow the same [formatting pattern](Version::is_same_pattern), as comparisons between
/// different patterns can produce results inconsistent with typical version semantics.
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Version<'s>(Box<[VersionPart<'s>]>);

impl<'s> Version<'s> {
	/// Chunks an arbitrary string into a version sequence.
	pub fn from(text: &'s str) -> Version<'s> {
		Version(
			text.as_bytes()
				.chunk_by(|a, b| a.is_ascii_digit() == b.is_ascii_digit())
				.map(|chunk| str::from_utf8(chunk).unwrap())
				.map(|chunk| {
					if chunk.as_bytes()[0].is_ascii_digit() {
						VersionPart::Num(DigitStr::new(chunk))
					} else {
						VersionPart::Str(chunk)
					}
				})
				.collect(),
		)
	}

	/// Determines whether two version sequences follow the same formatting pattern.
	///
	/// This is true if the order and count of digit and non-digit parts matches between the two
	/// strings, and the non-digit portions of the version strings are equal.
	///
	/// Examples of strings with the same formatting pattern include:
	///
	///   - `v1.0.10` and `v3.44.247`
	///   - `2.1` and `10.0`
	///   - `1.0.0-rc.1` and `2.0.0-rc.3`
	///
	/// Examples of strings with different formatting patterns include:
	///
	///   - `latest` and `2025-11-12T13-14-15Z`
	///   - `.34` and `0.34`
	///   - `1.1.0` and `v1.1.0`
	///   - `2.0.0-alpha.1` and `2.0.0-beta.1`
	///
	/// Formatting patterns may be useful to select stable version strings in a list of arbitrary
	/// strings, such as a list of container registry tags that mixes semantic versions with
	/// `latest`, Git SHAs, timestamps, etc. As the latter examples attempt to demonstrate, they're
	/// not useful for comparing version strings in general.
	pub fn is_same_pattern(&self, other: &Self) -> bool {
		use VersionPart::{Num, Str};

		let (mut a, mut b) = (self.0.iter(), other.0.iter());
		loop {
			match (a.next(), b.next()) {
				(None, None) => return true,
				(Some(Num(_)), Some(Num(_))) => {}
				(Some(Str(a)), Some(Str(b))) => {
					if a != b {
						return false;
					}
				}
				_ => return false,
			}
		}
	}
}

impl<'s> Deref for Version<'s> {
	type Target = [VersionPart<'s>];

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl Display for Version<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.iter().try_for_each(|part| part.fmt(f))
	}
}

/// A single part in a [`Version`] string.
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum VersionPart<'s> {
	Num(DigitStr<'s>),
	Str(&'s str),
}

impl Display for VersionPart<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			VersionPart::Num(i) => i.fmt(f),
			VersionPart::Str(s) => s.fmt(f),
		}
	}
}

/// A string of ASCII digits that compares numerically rather than lexicographically.
#[derive(Debug, Eq, PartialEq)]
pub struct DigitStr<'s>(&'s str);

impl<'s> DigitStr<'s> {
	/// Returns a `DigitStr` wrapping the provided string.
	///
	/// # Panics
	///
	/// If `text` contains characters other than ASCII digits.
	pub fn new(text: &'s str) -> DigitStr<'s> {
		if text.as_bytes().iter().all(u8::is_ascii_digit) {
			DigitStr(text)
		} else {
			panic!("DigitStr should only contain ASCII digit characters");
		}
	}
}

impl Ord for DigitStr<'_> {
	fn cmp(&self, other: &Self) -> cmp::Ordering {
		let a = self.0.trim_start_matches('0');
		let b = other.0.trim_start_matches('0');

		if a.len() == b.len() {
			a.cmp(b)
		} else {
			a.len().cmp(&b.len())
		}
	}
}

impl PartialOrd for DigitStr<'_> {
	fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
		Some(self.cmp(other))
	}
}

impl Display for DigitStr<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

#[cfg(test)]
mod tests {
	use std::cmp::Ordering::{self, Equal, Greater, Less};

	use rstest::rstest;

	use super::VersionPart::{Num, Str};
	use super::*;

	#[test]
	fn version_empty() {
		assert_eq!(&*Version::from(""), []);
	}

	#[test]
	fn version_from_latest() {
		assert_eq!(&*Version::from("latest"), [Str("latest")]);
	}

	#[test]
	fn version_from_unix() {
		assert_eq!(&*Version::from("1759807235"), [Num(DigitStr("1759807235"))]);
	}

	#[test]
	fn version_from_v_semver() {
		assert_eq!(
			&*Version::from("v1.2.3"),
			[
				Str("v"),
				Num(DigitStr("1")),
				Str("."),
				Num(DigitStr("2")),
				Str("."),
				Num(DigitStr("3"))
			]
		);
	}

	#[test]
	fn version_display() {
		assert_eq!(
			Version::from("v1.2-beta.13a").to_string().as_str(),
			"v1.2-beta.13a"
		);
	}

	#[rstest]
	// The specific patterns from the doc comment.
	#[case::doc_v_semver("v1.0.10", "v3.44.247")]
	#[case::doc_twopart("2.1", "10.0")]
	#[case::doc_rc("1.0.0-rc.1", "2.0.0-rc.3")]
	// Additional patterns for completeness.
	#[case::empty("", "")]
	#[case::latest("latest", "latest")]
	#[case::bare_semver("1.0.0", "13.5.3")]
	#[case::timestamp("1970-01-01T00-00-00Z", "2025-11-12T13-14-15Z")]
	fn version_patterns_same(#[case] a: &str, #[case] b: &str) {
		assert!(
			Version::from(a).is_same_pattern(&Version::from(b)),
			"{a} =~ {b}"
		);
	}

	#[rstest]
	// The specific patterns from the doc comment.
	#[case::doc_latest_ts("latest", "2025-11-12T13-14-15Z")]
	#[case::doc_leading_dot(".34", "0.34")]
	#[case::doc_semver_formats("1.1.0", "v1.1.0")]
	#[case::doc_alpha_beta("2.0.0-alpha.1", "2.0.0-beta.1")]
	// Additional patterns for completeness.
	#[case::latest_semver("latest", "v3.5.6")]
	fn version_patterns_different(#[case] a: &str, #[case] b: &str) {
		assert!(
			!Version::from(a).is_same_pattern(&Version::from(b)),
			"{a} !~ {b}"
		);
	}

	#[rstest]
	// Specific cases from the doc comment.
	#[case::doc_semver_major_less("v1.99.99", Less, "v2.0.0")]
	#[case::doc_leading_zero_maj_min("v1.050", Equal, "v1.50")]
	#[case::doc_surprise_beta_stable("v1.00", Less, "v1.0-beta.1")]
	#[case::doc_surprise_semver_v_bare("2.0.0", Less, "v1.0.0")]
	// Additional basic cases.
	#[case::empty_vs_semver("", Less, "1.0.0")]
	#[case::empty_vs_latest("", Less, "latest")]
	#[case::semver_basic_equal("2.0.0", Equal, "2.0.0")]
	#[case::semver_major_greater("ver 2.0.0", Greater, "ver 1.99.99")]
	#[case::no_digits("latest", Less, "main")]
	#[case::only_digits_same("00042", Equal, "42")]
	#[case::only_digits_diff("00123", Less, "12300")]
	fn version_ordering(#[case] a: &str, #[case] ord: Ordering, #[case] b: &str) {
		assert_eq!(ord, Version::from(a).cmp(&Version::from(b)), "{a} ~ {b}");
	}

	#[test]
	#[should_panic]
	fn digitstr_invalid() {
		DigitStr::new("hello");
	}
}
