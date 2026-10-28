//! Declarative WinCE CabWiz INF packages. No host installer/tool is executed.
use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;

type Sections = BTreeMap<String, Vec<String>>;
fn text(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) {
        if bytes.len() % 2 != 0 {
            return Err("Truncated UTF-16 INF".into());
        }
        return String::from_utf16(
            &bytes[2..]
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| "Invalid UTF-16 INF".into());
    }
    if bytes.is_ascii() {
        return Ok(String::from_utf8_lossy(bytes).into_owned());
    }
    std::str::from_utf8(bytes)
        .map(|s| s.trim_start_matches('\u{feff}').to_owned())
        .map_err(|_| "Unsupported INF text encoding (expected ASCII, UTF-8 or UTF-16LE)".into())
}
fn sections(bytes: &[u8]) -> Result<Sections> {
    if bytes.len() > 1024 * 1024 {
        return Err("CabWiz INF exceeds metadata limit".into());
    }
    let mut out = Sections::new();
    let mut current = String::new();
    for line in text(bytes)?.lines() {
        let mut quoted = false;
        let end = line
            .char_indices()
            .find_map(|(i, c)| {
                if c == '"' {
                    quoted = !quoted;
                }
                (c == ';' && !quoted).then_some(i)
            })
            .unwrap_or(line.len());
        let line = line[..end].trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].trim().to_ascii_lowercase();
            out.entry(current.clone()).or_default();
        } else {
            if current.is_empty() || line.ends_with('\\') {
                return Err("Unsupported INF continuation/header".into());
            }
            out.entry(current.clone())
                .or_default()
                .push(line.to_owned());
        }
    }
    Ok(out)
}
fn csv(value: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = value.trim().chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if quoted && chars.peek() == Some(&'"') {
                field.push('"');
                chars.next();
            } else {
                quoted = !quoted;
            }
        } else if c == ',' && !quoted {
            out.push(field.trim().to_owned());
            field.clear();
        } else {
            field.push(c);
        }
    }
    if quoted {
        return Err("Unterminated INF quote".into());
    }
    out.push(field.trim().to_owned());
    Ok(out)
}
fn pairs(s: &Sections, name: &str) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for line in s.get(&name.to_ascii_lowercase()).into_iter().flatten() {
        let (key, value) = line.split_once('=').ok_or("Expected INF key=value")?;
        let key = csv(key)?.join(",").to_ascii_lowercase();
        if out.insert(key, value.trim().to_owned()).is_some() {
            return Err("Duplicate INF key".into());
        }
    }
    Ok(out)
}
fn scalar(p: &HashMap<String, String>, key: &str) -> Result<String> {
    let fields = csv(p.get(key).ok_or_else(|| format!("Missing INF {key}"))?)?;
    if fields.len() != 1 {
        return Err(format!("Expected scalar INF {key}"));
    }
    Ok(fields[0].clone())
}
fn expand(value: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut value = value.to_owned();
    for _ in 0..16 {
        let mut out = String::new();
        let mut remaining = value.as_str();
        let mut changed = false;
        while let Some(start) = remaining.find('%') {
            out.push_str(&remaining[..start]);
            remaining = &remaining[start + 1..];
            let end = remaining.find('%').ok_or("Unterminated INF substitution")?;
            let key = remaining[..end].to_ascii_lowercase();
            if let Some(v) = vars.get(&key) {
                out.push_str(v);
                changed = true;
            } else if key
                .strip_prefix("ce")
                .is_some_and(|v| v.parse::<u32>().is_ok())
            {
                out.push('%');
                out.push_str(&remaining[..end]);
                out.push('%');
            } else {
                return Err(format!("Unknown INF substitution %{key}%"));
            }
            if out.len() > 65536 {
                return Err("INF substitution exceeds size limit".into());
            }
            remaining = &remaining[end + 1..];
        }
        out.push_str(remaining);
        if out.len() > 65536 {
            return Err("INF substitution exceeds size limit".into());
        }
        if !changed {
            return Ok(out);
        }
        value = out;
    }
    Err("Cyclic INF substitutions".into())
}
fn relative(value: &str) -> Result<String> {
    let value = value.replace('\\', "/");
    if value.starts_with('/')
        || value.contains(':')
        || value.split('/').any(|p| p == ".." || p == ".")
    {
        return Err("Unsafe INF source path".into());
    }
    Ok(value.trim_matches('/').to_owned())
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn number(s: &str) -> Result<u32> {
    if let Some(s) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(s, 16)
    } else {
        s.parse()
    }
    .map_err(|_| "Invalid INF number".into())
}
pub(super) fn is_manifest(file: &Cabinet) -> bool {
    file.name.to_ascii_lowercase().ends_with(".inf")
        && sections(&file.bytes).is_ok_and(|s| {
            pairs(&s, "version").is_ok_and(|p| {
                scalar(&p, "cesignature").is_ok_and(|s| s.eq_ignore_ascii_case("$Windows CE$"))
            })
        })
}
pub(super) fn extract(files: &[Cabinet], budget: &mut usize) -> Result<Vec<Cabinet>> {
    let mut result = Vec::new();
    for manifest in files.iter().filter(|f| is_manifest(f)) {
        let s = sections(&manifest.bytes)?;
        let mut vars = HashMap::new();
        for (k, v) in pairs(&s, "strings")?
            .into_iter()
            .chain(pairs(&s, "cestrings")?)
        {
            vars.insert(k, csv(&v)?.join(","));
        }
        let app = expand(vars.get("appname").ok_or("Missing INF AppName")?, &vars)?;
        let install = expand(
            vars.get("installdir").ok_or("Missing INF InstallDir")?,
            &vars,
        )?;
        let variants: Vec<_> = s
            .keys()
            .filter_map(|k| k.strip_prefix("cedevice."))
            .collect();
        let variants = if variants.is_empty() {
            vec![""]
        } else {
            variants
        };
        for variant in variants {
            if result.len() >= 32 {
                return Err("Too many CabWiz variants".into());
            }
            let decorated = |base: &str| {
                if variant.is_empty() {
                    base.to_owned()
                } else {
                    format!("{base}.{variant}")
                }
            };
            let merge = |base: &str| -> Result<HashMap<String, String>> {
                let mut p = pairs(&s, base)?;
                if !variant.is_empty() {
                    p.extend(pairs(&s, &decorated(base))?);
                }
                Ok(p)
            };
            let device = merge("cedevice")?;
            let processor = device
                .get("processortype")
                .map(|v| number(&csv(v)?[0]))
                .transpose()?
                .unwrap_or(0);
            let commands = merge("defaultinstall")?;
            for key in commands.keys() {
                if !["copyfiles", "addreg", "ceshortcuts"].contains(&key.as_str()) {
                    return Err(format!("Unsupported CabWiz directive {key}"));
                }
            }
            let sources = merge("sourcedisksfiles")?;
            let disks = merge("sourcedisksnames")?;
            let dirs = merge("destinationdirs")?;
            let mut payloads: Vec<(String, &[u8])> = Vec::new();
            let mut destinations = HashSet::new();
            let mut setup=format!("<wap-provisioningdoc>\n<characteristic type=\"Install\">\n<parm name=\"AppName\" value=\"{}\" />\n<parm name=\"InstallDir\" value=\"{}\" />\n<parm name=\"ProcessorType\" value=\"{processor}\" />\n</characteristic>\n<characteristic type=\"FileOperation\">\n",xml(&app),xml(&install));
            for group in csv(commands.get("copyfiles").ok_or("Missing INF CopyFiles")?)? {
                let key = group.to_ascii_lowercase();
                if group.starts_with('@') {
                    return Err("Direct INF CopyFiles entries not supported".into());
                }
                let dest = csv(dirs
                    .get(&key)
                    .or_else(|| dirs.get("defaultdestdir"))
                    .ok_or("Missing INF DestinationDirs entry")?)?;
                if dest.len() != 2 || dest[0] != "0" {
                    return Err("Unsupported INF destination directory ID".into());
                }
                let dest = expand(&dest[1], &vars)?;
                if dest.contains("..") {
                    return Err("Unsafe INF destination".into());
                }
                setup.push_str(&format!("<characteristic type=\"{}\">\n", xml(&dest)));
                for line in s.get(&key).into_iter().flatten() {
                    if setup.len() > 1024 * 1024 || payloads.len() >= 4096 {
                        return Err("CabWiz package metadata exceeds limit".into());
                    }
                    let fields = csv(line)?;
                    if fields.is_empty() || fields[0].is_empty() || fields.len() > 4 {
                        return Err("Invalid INF CopyFiles row".into());
                    }
                    if fields.get(2).is_some_and(|v| !v.is_empty())
                        || fields.get(3).is_some_and(|v| !v.is_empty() && v != "0")
                    {
                        return Err("Unsupported INF copy flags".into());
                    }
                    let name = expand(&fields[0], &vars)?;
                    if relative(&name)?.contains('/') {
                        return Err("INF destination must be a filename".into());
                    }
                    let source = fields
                        .get(1)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(&fields[0]);
                    let source = expand(source, &vars)?;
                    let source_fields = csv(sources
                        .get(&source.to_ascii_lowercase())
                        .ok_or_else(|| format!("Missing INF source {source}"))?)?;
                    if source_fields.len() != 1 {
                        return Err("Unsupported INF source flags".into());
                    }
                    let disk = csv(disks
                        .get(&source_fields[0])
                        .ok_or("Missing INF source disk")?)?;
                    let directory = disk.get(3).ok_or("Missing INF source directory")?;
                    let directory = expand(directory, &vars)?;
                    let wanted = relative(&if directory.is_empty() {
                        source.clone()
                    } else {
                        format!("{directory}/{source}")
                    })?;
                    let prefix = manifest.name.replace('\\', "/");
                    let prefix = prefix.rsplit_once('/').map(|p| p.0).unwrap_or("");
                    let wanted = if prefix.is_empty() {
                        wanted
                    } else {
                        format!("{prefix}/{wanted}")
                    };
                    let mut matches = files
                        .iter()
                        .filter(|f| f.name.replace('\\', "/").eq_ignore_ascii_case(&wanted));
                    let file = matches
                        .next()
                        .ok_or_else(|| format!("Missing CabWiz payload {wanted}"))?;
                    if matches.next().is_some() {
                        return Err("Ambiguous CabWiz source".into());
                    }
                    if !destinations.insert(format!("{dest}\\{name}").to_ascii_lowercase()) {
                        return Err("Duplicate CabWiz destination".into());
                    }
                    let short = format!("F{:07}.bin", payloads.len());
                    payloads.push((short.clone(), &file.bytes));
                    setup.push_str(&format!("<characteristic type=\"{}\"><characteristic type=\"Extract\"><parm name=\"Source\" value=\"{short}\" /></characteristic></characteristic>\n",xml(&name)));
                }
                setup.push_str("</characteristic>\n");
            }
            setup.push_str("</characteristic>\n");
            if let Some(groups) = commands.get("ceshortcuts") {
                for group in csv(groups)? {
                    for line in s.get(&group.to_ascii_lowercase()).into_iter().flatten() {
                        let f = csv(line)?;
                        if f.len() < 3 || f[1] != "0" {
                            return Err("Unsupported INF shortcut".into());
                        }
                        let target = expand(&format!("{install}\\{}", f[2]), &vars)?;
                        setup.push_str(&format!("<characteristic type=\"Shortcut\"><parm name=\"Source\" value=\"{}\" /></characteristic>\n",xml(&target)));
                    }
                }
            }
            setup.push_str("<characteristic type=\"Registry\">\n");
            if let Some(groups) = commands.get("addreg") {
                for group in csv(groups)? {
                    for line in s.get(&group.to_ascii_lowercase()).into_iter().flatten() {
                        let f = csv(line)?;
                        if f.len() != 5
                            || !["HKLM", "HKCU", "HKCR"]
                                .contains(&f[0].to_ascii_uppercase().as_str())
                        {
                            return Err("Unsupported INF registry row".into());
                        }
                        let flags = number(&f[3])?;
                        let kind = match flags {
                            0 => "string",
                            0x10001 => "integer",
                            _ => return Err("Unsupported INF registry flags".into()),
                        };
                        let value = expand(&f[4], &vars)?;
                        let value = if flags == 0x10001 {
                            number(&value)?.to_string()
                        } else {
                            value
                        };
                        setup.push_str(&format!("<characteristic type=\"{}\\{}\"><parm name=\"{}\" value=\"{}\" datatype=\"{kind}\" /></characteristic>\n",xml(&f[0]),xml(&expand(&f[1],&vars)?),xml(&expand(&f[2],&vars)?),xml(&value)));
                    }
                }
            }
            setup.push_str("</characteristic>\n</wap-provisioningdoc>\n");
            if payloads.is_empty() {
                return Err("CabWiz package contains no files".into());
            }
            let mut builder = cab::CabinetBuilder::new();
            let folder = builder.add_folder(cab::CompressionType::None);
            folder.add_file("_setup.xml");
            for (name, _) in &payloads {
                folder.add_file(name);
            }
            let estimate = payloads
                .iter()
                .try_fold(setup.len(), |n, p| n.checked_add(p.1.len() + 128))
                .and_then(|n| n.checked_add(8192 + (n / 32768 + 1) * 16))
                .ok_or("CabWiz size overflow")?;
            charge(estimate, budget)?;
            let mut writer = builder
                .build(Cursor::new(Vec::new()))
                .map_err(|e| e.to_string())?;
            {
                let mut file = writer
                    .next_file()
                    .map_err(|e| e.to_string())?
                    .ok_or("Missing CAB setup entry")?;
                file.write_all(setup.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            for (_, data) in payloads {
                let mut file = writer
                    .next_file()
                    .map_err(|e| e.to_string())?
                    .ok_or("Missing CAB file entry")?;
                file.write_all(data).map_err(|e| e.to_string())?;
            }
            let bytes = writer.finish().map_err(|e| e.to_string())?.into_inner();
            if bytes.len() > estimate {
                return Err("CabWiz output exceeded allocation bound".into());
            }
            let name = format!(
                "{}-{}.cab",
                manifest.name,
                if variant.is_empty() {
                    "default"
                } else {
                    variant
                }
            );
            result.push(Cabinet { name, bytes });
        }
    }
    if result.is_empty() {
        return Err("No WinCE CabWiz manifest found".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(inf: &str) -> Vec<Cabinet> {
        vec![
            Cabinet {
                name: "package.inf".into(),
                bytes: inf.as_bytes().to_vec(),
            },
            Cabinet {
                name: "arm/game.exe".into(),
                bytes: b"test executable".to_vec(),
            },
        ]
    }
    const INF: &str = "[Version]\nCESignature=\"$Windows CE$\"\n[CEStrings]\nAppName=Example\nInstallDir=%CE1%\\Example\n[CEDevice.ARM]\nProcessorType=2577\n[SourceDisksNames.ARM]\n1=,Files,,arm\n[SourceDisksFiles]\ngame.exe=1\n[DefaultInstall]\nCopyFiles=Files\nAddReg=Registry\n[DestinationDirs]\nFiles=0,%InstallDir%\n[Files]\ngame.exe\n[Registry]\nHKLM,Software\\%AppName%,Version,0x10001,1\n";
    #[test]
    fn reconstructs_declared_files_registry_and_processor_metadata() {
        let mut budget = LIMIT;
        let cabinets = extract(&sample(INF), &mut budget).unwrap();
        assert_eq!(cabinets.len(), 1);
        let mut archive = cab::Cabinet::new(Cursor::new(&cabinets[0].bytes)).unwrap();
        let mut setup = Vec::new();
        archive
            .read_file("_setup.xml")
            .unwrap()
            .read_to_end(&mut setup)
            .unwrap();
        let script = pocket_cab::WinCeSetupScript::parse_bytes(&setup);
        assert_eq!(script.processor_type, Some(2577));
        assert_eq!(script.registry[0].dword, Some(1));
        assert_eq!(script.registry[0].key, "HKLM\\Software\\Example");
        assert!(script.renames[0].1.ends_with("Example\\game.exe"));
        let mut bytes = Vec::new();
        archive
            .read_file(&script.renames[0].0)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"test executable");
    }
    #[test]
    fn refuses_missing_sources_unsafe_paths_unknown_directives_and_exhausted_budget() {
        for bad in [
            INF.replace("1=,Files,,arm", "1=,Files,,..\\arm"),
            INF.replace("game.exe=1", "other.exe=1"),
            INF.replace(
                "CopyFiles=Files",
                "CopyFiles=Files\nRunPreSetupCommands=Run",
            ),
            INF.replace("0x10001,1", "0x00010000,a"),
        ] {
            assert!(extract(&sample(&bad), &mut { LIMIT }).is_err());
        }
        assert!(extract(&sample(INF), &mut 1).is_err());
    }
    #[test]
    fn quoted_comments_and_case_insensitive_substitutions_are_preserved() {
        assert_eq!(
            csv("\"a,b\",\"a\"\"b\",,0").unwrap(),
            ["a,b", "a\"b", "", "0"]
        );
        let s = sections(b"[Strings]\nName=\"one;two\" ; comment\n").unwrap();
        let vars = pairs(&s, "strings").unwrap();
        assert_eq!(csv(&vars["name"]).unwrap(), ["one;two"]);
        let mut vars = HashMap::new();
        vars.insert("appname".into(), "Example".into());
        assert_eq!(expand("%APPNAME%", &vars).unwrap(), "Example");
        vars.insert("appname".into(), "%AppName%".into());
        assert!(expand("%AppName%", &vars).is_err());
    }
    #[test]
    fn supplied_cabwiz_installer_offers_each_architecture_and_imports_arm() {
        let Some(path) = std::env::var_os("POCKET_CABWIZ_SAMPLE") else {
            return;
        };
        let root = tempfile::tempdir().unwrap();
        let library = crate::Library::open(root.path()).unwrap();
        let mut prepared = library
            .prepare_import(std::path::PathBuf::from(path))
            .unwrap();
        assert_eq!(prepared.cabinet_choices().len(), 3);
        let mut architectures = prepared
            .cabinet_choices()
            .iter()
            .map(|c| c.architecture.clone())
            .collect::<Vec<_>>();
        architectures.sort();
        assert_eq!(architectures, ["ARM", "MIPS", "SH3"]);
        let arm = prepared
            .cabinet_choices()
            .iter()
            .find(|c| c.architecture == "ARM")
            .unwrap()
            .index;
        prepared.choose_cabinet(arm).unwrap();
        let entry = &prepared.entries()[0];
        assert_eq!(entry.display_name, "Strip4You");
        assert!(entry
            .executable
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with("strip4you.exe"));
        assert_eq!(
            entry.registry.iter().filter(|r| r.dword == Some(1)).count(),
            1
        );
        let exe = entry.executable_path(prepared.staging.path());
        let bytes = std::fs::read(&exe).unwrap();
        let pe = u32_at(&bytes, 60).unwrap();
        assert_eq!(u16_at(&bytes, pe + 4), Some(0x1c0)); // actual ARM payload
        let directory = exe.parent().unwrap();
        assert!(directory.join("Menu.mod").is_file());
        assert!(directory.join("gd101.dll").is_file());
        assert!(!directory.join("MAKECAB.EXE").exists());

        // The selected entry's files remain in isolated import staging until confirmation.
        assert!(library.games().is_empty());
    }
}
