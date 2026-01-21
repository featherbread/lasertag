use std::error::Error;
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::iter;
use std::process;
use std::sync::{Arc, LazyLock};

use clap::Parser;
use oci_client::client::ClientConfig;
use oci_client::errors::OciDistributionError;
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use tokio::sync::Semaphore;

mod version;

use version::Version;

#[derive(clap::Parser)]
#[command(about)]
struct Cli {
	/// The images to check
	#[arg(value_parser = |raw: &str| raw.parse::<Reference>().map(Arc::new))]
	#[arg(required = true)]
	images: Vec<Arc<Reference>>,

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
				get_latest_similar_tag(&image).await
			})
		})
		.collect();

	let mut stdout = pipecheck::wrap(io::stdout().lock());
	let mut stderr = pipecheck::wrap(io::stderr().lock());
	let mut has_error = false;
	for (image, task) in iter::zip(cli.images, tasks) {
		match task.await.unwrap() {
			Ok(latest_tag) => {
				if !cli.differences || image.tag() != Some(&latest_tag) {
					let _ = writeln!(stdout, "{}\t{}", image, latest_tag);
				}
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
	static CLIENT: LazyLock<Client> = LazyLock::new(|| {
		Client::new(ClientConfig {
			user_agent: build_user_agent(),
			..Default::default()
		})
	});

	let client = &*CLIENT;
	let mut all_tags: Vec<String> = vec![];
	loop {
		let result = client
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

#[derive(Debug)]
enum TagError {
	ImageMissingTag,
	NoSimilarTag,
	Registry(OciDistributionError),
}

impl Error for TagError {}

impl Display for TagError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			TagError::ImageMissingTag => f.write_str("image reference has no tag to match on"),
			TagError::NoSimilarTag => f.write_str("no similar tag format found in registry"),
			TagError::Registry(err) => err.fmt(f),
		}
	}
}

impl From<OciDistributionError> for TagError {
	fn from(err: OciDistributionError) -> Self {
		Self::Registry(err)
	}
}
