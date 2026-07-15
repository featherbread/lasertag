use std::fmt::{self, Display};
use std::io::{self, Write};
use std::iter;
use std::process;
use std::str::FromStr;
use std::sync::{Arc, LazyLock};

use clap::Parser;
use oci_client::client::ClientConfig;
use oci_client::errors::OciDistributionError;
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, ParseError, Reference};
use thiserror::Error;
use tokio::sync::Semaphore;

mod version;

use version::Version;

#[derive(clap::Parser)]
#[command(about)]
struct Cli {
	/// The images to check
	#[arg(value_parser = |raw: &str| raw.parse::<Input>().map(Arc::new))]
	#[arg(required = true)]
	images: Vec<Arc<Input>>,

	/// Only print images whose newest tag differs
	#[arg(short = 'd')]
	#[arg(long)]
	differences: bool,

	/// The maximum number of images to check concurrently at any one time
	#[arg(default_value_t = 5)]
	#[arg(long)]
	concurrency: usize,
}

#[tokio::main]
async fn main() {
	let cli = Cli::parse();

	let sema = Arc::new(Semaphore::new(cli.concurrency));
	let tasks: Vec<_> = cli
		.images
		.iter()
		.map(|image| {
			let sema = Arc::clone(&sema);
			let image = Arc::clone(image);
			tokio::spawn(async move {
				let _permit = sema.acquire().await.unwrap();
				get_latest_similar_image(&image.parsed).await
			})
		})
		.collect();

	let mut stdout = pipecheck::wrap(io::stdout().lock());
	let mut stderr = pipecheck::wrap(io::stderr().lock());
	let mut has_error = false;
	for (image, task) in iter::zip(cli.images, tasks) {
		match task.await.unwrap() {
			Ok(latest) if cli.differences && image.parsed.tag() == Some(&latest.tag) => continue,
			Ok(latest) => {
				let _ = writeln!(stdout, "{}\t{}", image, latest);
			}
			Err(err) => {
				let _ = writeln!(stderr, "{}\t{}", image, err);
				has_error = true;
			}
		}
	}
	if has_error {
		process::exit(1);
	}
}

struct Input {
	original: String,
	parsed: Reference,
}

impl FromStr for Input {
	type Err = ParseError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let parsed = Reference::from_str(s)?;
		let original = s.to_owned();
		Ok(Input { original, parsed })
	}
}

impl Display for Input {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.original)
	}
}

struct Latest {
	tag: String,
	digest: Option<String>,
}

impl Display for Latest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match &self.digest {
			Some(digest) => write!(f, "{}@{}", self.tag, digest),
			None => f.write_str(&self.tag),
		}
	}
}

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
	Client::new(ClientConfig {
		user_agent: build_user_agent(),
		..Default::default()
	})
});

async fn get_latest_similar_image(base_image: &Reference) -> TagResult<Latest> {
	let tag = get_latest_similar_tag(base_image).await?;

	let digest = match base_image.digest() {
		None => None,
		Some(_) => {
			let latest_image = Reference::with_tag(
				base_image.registry().to_owned(),
				base_image.repository().to_owned(),
				tag.clone(),
			);
			let digest = CLIENT
				.fetch_manifest_digest(&latest_image, &RegistryAuth::Anonymous)
				.await?;

			Some(digest)
		}
	};

	Ok(Latest { tag, digest })
}

async fn get_latest_similar_tag(image: &Reference) -> TagResult<String> {
	let start_version = Version::from(image.tag().ok_or(TagError::ImageMissingTag)?);

	let all_tags = list_all_tags(image).await?;
	let mut versions: Vec<_> = all_tags
		.iter()
		.map(|tag| Version::from(tag))
		.filter(|version| version.is_same_pattern(&start_version))
		.collect();

	versions.sort();
	Ok(versions.last().ok_or(TagError::NoSimilarTag)?.to_string())
}

async fn list_all_tags(image: &Reference) -> TagResult<Vec<String>> {
	let mut all_tags: Vec<String> = vec![];
	loop {
		let result = CLIENT
			.list_tags(
				image,
				&RegistryAuth::Anonymous,
				Some(1000),
				all_tags.last().map(|tag| tag.as_str()),
			)
			.await;

		match result {
			Ok(result) if result.tags.is_empty() => return Ok(all_tags),
			Ok(result) => all_tags.extend(result.tags),
			Err(err) => return Err(err.into()),
		};
	}
}

const fn build_user_agent() -> &'static str {
	const NAME: &str = env!("CARGO_PKG_NAME");
	const VERSION: &str = env!("CARGO_PKG_VERSION");
	const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

	const_format::formatcp!("{NAME}/{VERSION} (+{REPOSITORY})")
}

type TagResult<T> = Result<T, TagError>;

#[derive(Error, Debug)]
enum TagError {
	#[error("image reference has no tag to match on")]
	ImageMissingTag,

	#[error("no similar tag format found in registry")]
	NoSimilarTag,

	#[error(transparent)]
	Registry(#[from] OciDistributionError),
}
