//! Buildroot .config reader: raw key/value access plus a distilled summary
//! of the facts the report cares about.

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct BrConfig {
    values: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigSummary {
    pub defconfig: Option<String>,
    pub arch: Option<String>,
    pub target_cpu: Option<String>,
    pub libc: Option<String>,
    pub kernel_version: Option<String>,
    pub rootfs_types: Vec<String>,
    pub squashfs_compression: Option<String>,
    pub post_image_scripts: Option<String>,
}

impl BrConfig {
    pub fn parse(text: &str) -> Self {
        let mut values = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().trim_matches('"').to_string();
            values.insert(k.trim().to_string(), v);
        }
        BrConfig { values }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    /// Every option that is set, by key.
    ///
    /// Sorted, because the source is a hash map and a report that reordered
    /// itself between runs would make every diff noise. Options that are not
    /// set never appear: Buildroot writes those as comments, so "absent" and
    /// "not set" are the same thing here.
    pub fn options(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .values
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn is_y(&self, key: &str) -> bool {
        self.get(key) == Some("y")
    }

    fn nonempty(&self, key: &str) -> Option<String> {
        self.get(key)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
    }

    pub fn summary(&self) -> ConfigSummary {
        let libc = if self.is_y("BR2_TOOLCHAIN_USES_UCLIBC") {
            Some("uclibc".to_string())
        } else if self.is_y("BR2_TOOLCHAIN_USES_MUSL") {
            Some("musl".to_string())
        } else if self.is_y("BR2_TOOLCHAIN_USES_GLIBC") {
            Some("glibc".to_string())
        } else {
            None
        };

        let kernel_version = self
            .nonempty("BR2_LINUX_KERNEL_CUSTOM_VERSION_VALUE")
            .or_else(|| self.nonempty("BR2_LINUX_KERNEL_VERSION"));

        let mut rootfs_types = Vec::new();
        for (key, name) in [
            ("BR2_TARGET_ROOTFS_SQUASHFS", "squashfs"),
            ("BR2_TARGET_ROOTFS_JFFS2", "jffs2"),
            ("BR2_TARGET_ROOTFS_UBI", "ubi"),
            ("BR2_TARGET_ROOTFS_UBIFS", "ubifs"),
            ("BR2_TARGET_ROOTFS_EXT2", "ext2"),
            ("BR2_TARGET_ROOTFS_CPIO", "cpio"),
            ("BR2_TARGET_ROOTFS_TAR", "tar"),
            ("BR2_TARGET_ROOTFS_INITRAMFS", "initramfs"),
        ] {
            if self.is_y(key) {
                rootfs_types.push(name.to_string());
            }
        }

        let squashfs_compression = [
            ("BR2_TARGET_ROOTFS_SQUASHFS4_GZIP", "gzip"),
            ("BR2_TARGET_ROOTFS_SQUASHFS4_LZ4", "lz4"),
            ("BR2_TARGET_ROOTFS_SQUASHFS4_LZMA", "lzma"),
            ("BR2_TARGET_ROOTFS_SQUASHFS4_LZO", "lzo"),
            ("BR2_TARGET_ROOTFS_SQUASHFS4_XZ", "xz"),
            ("BR2_TARGET_ROOTFS_SQUASHFS4_ZSTD", "zstd"),
        ]
        .iter()
        .find(|(k, _)| self.is_y(k))
        .map(|(_, n)| n.to_string());

        ConfigSummary {
            defconfig: self
                .nonempty("BR2_DEFCONFIG")
                .map(|d| d.rsplit('/').next().unwrap_or(&d).to_string()),
            arch: self.nonempty("BR2_ARCH"),
            target_cpu: self.nonempty("BR2_GCC_TARGET_ARCH"),
            libc,
            kernel_version,
            rootfs_types,
            squashfs_compression,
            post_image_scripts: self.nonempty("BR2_ROOTFS_POST_IMAGE_SCRIPT"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# comment
BR2_ARCH="mipsel"
BR2_GCC_TARGET_ARCH="mips32r2"
BR2_TOOLCHAIN_USES_UCLIBC=y
BR2_LINUX_KERNEL_CUSTOM_VERSION_VALUE="3.10.14"
BR2_TARGET_ROOTFS_SQUASHFS=y
BR2_TARGET_ROOTFS_SQUASHFS4_XZ=y
BR2_DEFCONFIG="/x/configs/foo_defconfig"
# BR2_TARGET_ROOTFS_EXT2 is not set
"#;

    #[test]
    fn summary_extracts_facts() {
        let s = BrConfig::parse(SAMPLE).summary();
        assert_eq!(s.arch.as_deref(), Some("mipsel"));
        assert_eq!(s.libc.as_deref(), Some("uclibc"));
        assert_eq!(s.kernel_version.as_deref(), Some("3.10.14"));
        assert_eq!(s.rootfs_types, vec!["squashfs"]);
        assert_eq!(s.squashfs_compression.as_deref(), Some("xz"));
        assert_eq!(s.defconfig.as_deref(), Some("foo_defconfig"));
    }
}
