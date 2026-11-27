//! The one Roblox Studio version rustudio targets.
//!
//! Everything version-coupled is pinned here: the Studio content rustudio
//! downloads (CoreScripts, PlayerModule, ReflectionMetadata, textures…), the
//! bundled `api-dump.json` (that version's own `-API` dump) and the version
//! rustudio reports to scripts and to Roblox's web APIs. Downloads are checked
//! against the SHA-256 below, so a CDN change or a newer Studio never slips in
//! silently. To move to a new Studio: dump its API (`RobloxStudioBeta.exe -API
//! api-dump.json`), regenerate the stubs (`tools/gen_stubs.py`), and update
//! every constant here from that version's packages.

/// Studio's version (`version()`, the web User-Agent).
pub const VERSION: &str = "0.748.0.7481526";
/// Its deployment on `setup.rbxcdn.com`.
pub const DEPLOYMENT: &str = "version-8d7a1009c17e4ff8";

/// SHA-256 of each deployment package rustudio downloads.
pub const PACKAGES: &[(&str, &str)] = &[
    ("content-avatar.zip", "d03ca6b96bdcdc44dc048fb30ede3a028f1123cb18f367b84f952522cc152610"),
    ("content-configs.zip", "e3d515573338c760fd32353a4397f18f4723bfb3dd2a2bc33dec4962bfa2091c"),
    ("content-fonts.zip", "21f2759a5148c427118911ae402be2a2def56f29bedbd33ace3ed2ed714422fb"),
    ("content-models.zip", "bd9d84b18399042e0478ccdc989081d48a82279a498b36efaa197a317e01df76"),
    ("content-platform-fonts.zip", "6620658a6288e6b58b8d86aaef4e7734e10778974e9a01d364fc7aac4d35f10b"),
    ("content-sky.zip", "c79f0eeb2bca4905c585c50333db3c6f727a554f5db82e64948f93668fbc18aa"),
    ("content-sounds.zip", "5a3aae5657899b6452004f3f43f122cb6c166c99b7261efbeb965da906626ed8"),
    ("content-studio_svg_textures.zip", "e6c427cdfc87cd0b7112dfaf9230fe16fd0cfced1bf2706aa6366668ea74d77a"),
    ("content-terrain.zip", "80e94cd6207f71b8a703037cbc769c43f00a211c94f37df49f8dbbf52ff63736"),
    ("content-textures2.zip", "9ee8b87863cdf22aa83cbb7410b5762720faf24afac78be37fc72f52a335bdff"),
    ("content-textures3.zip", "73c9d9a9333623a070ccc1e5f6cf325d83f1ef9ce35c4d255b66a1733f6e6a91"),
    ("extracontent-luapackages.zip", "a9e56494d74ab24df0e71c61c7ca7acbc46207633059022271fdd356cd736e51"),
    ("extracontent-models.zip", "45e833d2c19d2e642fe79beadc8091a30807e6739349305f3062cf93851ea597"),
    ("extracontent-scripts.zip", "0b310f6237691f637dcd367f3c91ee5ab2ae9964d2c1f178c92b67caa30e83be"),
    ("extracontent-textures.zip", "ff7796df7fae3235bd9f1dbaf22b01dc9424242ccf8d89e8114a815931f9051b"),
    ("studiocontent-models.zip", "0f5d7984686372cc18973f5b06207cf858ccea3cc689ce43c248b7a33d3b9d38"),
    ("studiocontent-textures.zip", "db6ea5f981df42672773517b9ed81758ba45c3afb582203729088d280ae83b02"),
];

/// SHA-256 of files rustudio takes alone out of a bigger package.
pub const FILES: &[(&str, &str)] = &[
    ("ReflectionMetadata.xml", "77212819c7ea27b1a52c9ee37efbb36f70be399f8782f325179f9d01a9adc51a"),
];

/// SHA-256 of the bundled `api-dump.json` (this version's `-API` dump), with
/// CRLF read as LF so a checkout's line endings don't matter.
pub const API_DUMP: &str = "ff0469d2704be22caa31efa35dc7240a498e2dc568e688e94d1f4315d8552235";

/// The `User-Agent` Studio sends to Roblox's game APIs.
pub fn user_agent() -> String {
    format!("RobloxStudio/WinInet RobloxApp/{VERSION} (GlobalDist; RobloxDirectDownload)")
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// The pinned hash of a package or single file, if it is one we pin.
pub fn expected(name: &str) -> Option<&'static str> {
    PACKAGES.iter().chain(FILES).find(|(n, _)| *n == name).map(|(_, h)| *h)
}

/// Checks `data` against the pinned hash of `name`. Unpinned names fail too,
/// so a new download can't be added without pinning it.
pub fn verify(name: &str, data: &[u8]) -> Result<(), String> {
    let want = expected(name).ok_or_else(|| format!("{name} is not pinned for Studio {VERSION}"))?;
    let got = sha256_hex(data);
    if got == want {
        Ok(())
    } else {
        Err(format!("{name} does not match Studio {VERSION} ({DEPLOYMENT}): sha256 {got}, expected {want}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_accepts_only_pinned_bytes() {
        assert!(verify("content-sky.zip", b"not the package").is_err());
        assert!(verify("unknown.zip", b"").unwrap_err().contains("not pinned"));
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn hashes_are_well_formed() {
        for (name, h) in PACKAGES.iter().chain(FILES).chain([&("api-dump.json", API_DUMP)]) {
            assert_eq!(h.len(), 64, "{name}");
            assert!(h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{name}");
        }
        assert!(DEPLOYMENT.starts_with("version-"));
    }
}
