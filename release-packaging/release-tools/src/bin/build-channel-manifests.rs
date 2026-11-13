//! Write the per-channel release manifests from the release inventory.
use du_release_tools::channel_manifests::{
    ChannelContext, InventoryDocument, build_channel_manifests,
};
use std::{collections::BTreeMap, env, error::Error, fs, io, path::PathBuf, process};

const USAGE: &str = "usage: build-channel-manifests <release-inventory.json> <output-directory> --repository OWNER/REPO [--tag vX.Y.Z] [--winget-identifier ID]";

fn main() {
    if let Err(error) = run() {
        eprintln!("build-channel-manifests: {error}");
        process::exit(1);
    }
}

fn invalid(message: &str) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidInput, format!("{message}\n{USAGE}")).into()
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut positional = Vec::new();
    let mut named = BTreeMap::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(name) = arg.strip_prefix("--") {
            let value = args
                .next()
                .ok_or_else(|| invalid("an option needs a value"))?;
            named.insert(name.to_owned(), value);
        } else {
            positional.push(arg);
        }
    }
    let [inventory_path, out] = positional.as_slice() else {
        return Err(invalid("expected an inventory and an output directory"));
    };
    let repository = named
        .get("repository")
        .ok_or_else(|| invalid("missing --repository"))?;
    let inventory: InventoryDocument = serde_json::from_str(&fs::read_to_string(inventory_path)?)?;
    let tag = named.get("tag").filter(|tag| !tag.is_empty());
    let identifier = named.get("winget-identifier").filter(|id| !id.is_empty());
    let context = ChannelContext {
        repository,
        tag: tag.map(String::as_str),
        winget_identifier: identifier.map(String::as_str),
    };
    let out = PathBuf::from(out);
    fs::create_dir_all(&out)?;
    for file in build_channel_manifests(&inventory, &context)? {
        fs::write(out.join(file.file_name), &file.contents)?;
        println!("wrote={}", file.file_name);
    }
    Ok(())
}
