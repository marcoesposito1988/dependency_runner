//! Utilities to parse Visual Studio project files

use crate::common::{readable_canonical_path, LookupError};
use fs_err as fs;
use std::collections::HashMap;
use std::path::PathBuf;

// Parsing of Visual Studio files

/// Debugging configurations
///
/// Extracted from a .vcxproj.user file
/// Grouped by configuration (e.g. Debug, Release, ...)
#[derive(Debug)]
pub struct VcxDebuggingConfiguration {
    pub configuration: String,
    pub path: Option<Vec<PathBuf>>,
    pub working_directory: Option<PathBuf>,
}

fn extract_config_from_node(n: &roxmltree::Node) -> Result<String, LookupError> {
    let configuration_re =
        regex::Regex::new(r"'\$\(Configuration\)(?:\|\$\(Platform\))?'=='(\w+)(?:\|\w+)?'")?;
    let configuration_condition_text = n
        .attribute("Condition")
        .ok_or_else(|| LookupError::ParseError("Failed to find Condition group".to_owned()))?;
    let config: String = configuration_re
        .captures_iter(configuration_condition_text)
        .next()
        .ok_or_else(|| LookupError::ParseError("Failed to find configuration name".to_owned()))?
        .get(1)
        .ok_or_else(|| LookupError::ParseError("Failed to find configuration name".to_owned()))?
        .as_str()
        .to_owned();
    Ok(config)
}

fn expand_variables(text: &str, project_file: &std::path::Path) -> String {
    let project_dir = project_file
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut project_dir_str = project_dir.to_str().unwrap_or("").to_string();
    if !project_dir_str.is_empty()
        && !project_dir_str.ends_with('\\')
        && !project_dir_str.ends_with('/')
    {
        project_dir_str.push(std::path::MAIN_SEPARATOR);
    }

    text.replace("$(ProjectDir)", &project_dir_str)
        .replace("$(SolutionDir)", &project_dir_str)
        .replace('\\', "/")
}

fn resolve_path(path: &str, project_file: &std::path::Path) -> PathBuf {
    let expanded = expand_variables(path, project_file);
    let p = std::path::Path::new(&expanded);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_file
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(p)
    }
}

fn extract_debugging_configuration_from_config_node(
    n: &roxmltree::Node,
    p: &std::path::Path,
) -> Result<VcxDebuggingConfiguration, LookupError> {
    let config = extract_config_from_node(n)?;

    let mut ret = VcxDebuggingConfiguration {
        configuration: config,
        path: None,
        working_directory: None,
    };

    if let Some(environment_node) = n
        .descendants()
        .find(|n| n.has_tag_name("LocalDebuggerEnvironment"))
    {
        let environment_text = environment_node.text().ok_or_else(|| {
            LookupError::ParseError("Failed to find LocalDebuggerEnvironment tag".to_owned())
        })?;
        let environment_variables: Vec<&str> = environment_text.lines().collect();
        let path_env_var = environment_variables
            .iter()
            .find(|l| l.trim_start().starts_with("PATH="))
            .ok_or_else(|| LookupError::ParseError("Failed to find PATH variable".to_owned()))?;
        let path_env_var_without_varname = path_env_var.strip_prefix("PATH=").ok_or_else(|| {
            LookupError::ParseError("Failed to find LocalDebuggerEnvironment tag".to_owned())
        })?;
        let path_entries = path_env_var_without_varname.split(';');
        let path_entries_expanded: Vec<PathBuf> = path_entries
            .filter(|s| !s.is_empty() && !s.contains("%PATH%"))
            .map(|s| resolve_path(s, p))
            .collect();
        ret.path = Some(path_entries_expanded);
    }

    if let Some(working_directory_node) = n
        .descendants()
        .find(|n| n.has_tag_name("LocalDebuggerWorkingDirectory"))
    {
        let working_directory_text = working_directory_node.text().ok_or_else(|| {
            LookupError::ParseError("Failed to find LocalDebuggerEnvironment tag".to_owned())
        })?;
        ret.working_directory = Some(resolve_path(working_directory_text, p));
    }

    Ok(ret)
}

// extracts the debugging configuration for an executable from the respective .vcxproj.user file
//
// A .vcxproj file can only relate to a single executable, but there may be specified many
// configurations (Debug, Release, ...)
// Extracted properties:
// - PATH variable (in the <LocalDebuggerEnvironment> property)
// - working directory (value of the <LocalDebuggerWorkingDirectory> property)
pub fn parse_vcxproj_user<P: AsRef<std::path::Path> + ?Sized>(
    p: &P,
) -> anyhow::Result<HashMap<String, VcxDebuggingConfiguration>> {
    let filecontent = fs::read_to_string(p)?;
    let doc = roxmltree::Document::parse(&filecontent)?;
    let project_node = doc
        .descendants()
        .find(|n| n.has_tag_name("Project"))
        .ok_or_else(|| LookupError::ParseError("Failed to find Project tag".to_owned()))?;
    let configuration_nodes: Vec<_> = project_node
        .descendants()
        .filter(|n| n.has_tag_name("PropertyGroup"))
        .collect();
    let debugging_config_per_config: HashMap<String, VcxDebuggingConfiguration> =
        configuration_nodes
            .iter()
            .map(|n| extract_debugging_configuration_from_config_node(n, p.as_ref()))
            .filter_map(Result::ok)
            .map(|e: VcxDebuggingConfiguration| (e.configuration.clone(), e))
            .collect();
    Ok(debugging_config_per_config)
}

/// Executable Information
///
/// Extracted from a .vcxproj file
/// Grouped by configuration (e.g. Debug, Release, ...)
/// Contains VcxDebuggingConfiguration extracted from the respective .vcxproj.user, if present
#[derive(Debug)]
pub struct VcxExecutableInformation {
    pub configuration: String,
    pub executable_path: PathBuf,
    pub debugging_configuration: Option<VcxDebuggingConfiguration>,
}

fn extract_tag(root: &roxmltree::Node, tag: &str, p: &std::path::Path) -> HashMap<String, String> {
    root.descendants()
        .filter(|n: &roxmltree::Node| n.has_tag_name(tag))
        .map(|n| {
            if let Some(od) = n.text() {
                extract_config_from_node(&n).map(|c| (c, expand_variables(od, p)))
            } else {
                Err(LookupError::ParseError(format!("Empty {tag} tag")))
            }
        })
        .filter_map(Result::ok)
        .collect()
}

// extracts relevant information for an executable from the respective .vcxproj file
//
// A .vcxproj file can only relate to a single executable, but there may be specified many
// configurations (Debug, Release, ...)
// Extracted properties:
// - output executable path (composed of <OutDir>, <TargetName>, <TargetExt>)
// - debugging information, if the respective .vcxproj.user is found next to the .vcxproj
pub fn parse_vcxproj<P: AsRef<std::path::Path> + ?Sized>(
    p: &P,
) -> anyhow::Result<HashMap<String, VcxExecutableInformation>> {
    let filecontent = fs::read_to_string(p)?;
    let doc = roxmltree::Document::parse(&filecontent)?;
    let project_node = doc
        .descendants()
        .find(|n| n.has_tag_name("Project"))
        .ok_or(LookupError::ParseError(format!(
            "Failed to find <Project> tag in file {}",
            readable_canonical_path(p.as_ref())?
        )))?;

    // extract the file path the config refers to (outdir + target name + extension)
    let outdir_per_config = extract_tag(&project_node, "OutDir", p.as_ref());
    let targetname_per_config = extract_tag(&project_node, "TargetName", p.as_ref());
    let targetext_per_config = extract_tag(&project_node, "TargetExt", p.as_ref());

    let configs: Vec<_> = outdir_per_config.keys().collect();

    let mut executable_info_per_config: HashMap<String, VcxExecutableInformation> = configs
        .iter()
        .map(|&c| {
            let (e_dir, e_name, e_ext) = (
                &outdir_per_config[c],
                &targetname_per_config[c],
                &targetext_per_config[c],
            );
            
            let parent_dir = resolve_path(e_dir, p.as_ref());

            (
                c.clone(),
                VcxExecutableInformation {
                    configuration: c.clone(),
                    executable_path: parent_dir.join(e_name.to_owned() + e_ext),
                    debugging_configuration: None,
                },
            )
        })
        .map(Some)
        .filter_map(|x| x)
        .collect();

    if let Some(parent_dir) = p.as_ref().parent() {
        if let Some(filename) = p.as_ref().file_name() {
            let mut vcxuser_filename = filename.to_owned();
            vcxuser_filename.push(".user");
            let vcxproj_user_path = parent_dir.join(vcxuser_filename);
            if vcxproj_user_path.exists() {
                if let Ok(debugging_configuration_per_config) =
                    parse_vcxproj_user(&vcxproj_user_path)
                {
                    for (c, dc) in debugging_configuration_per_config {
                        if let Some(outdir) = executable_info_per_config.get_mut(&c) {
                            outdir.debugging_configuration = Some(dc);
                        }
                    }
                }
            }
        }
    }

    Ok(executable_info_per_config)
}

#[cfg(test)]
mod tests {
    use crate::common::LookupError;

    #[test]
    fn vcxproj() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        let vcxproj_path = d.join(
            "test_data/test_project1/DepRunTest/build-vcxproj-user/DepRunTest/DepRunTest.vcxproj",
        );
        let p = super::parse_vcxproj(&vcxproj_path)?;

        let mut config: Vec<&String> = p.keys().collect();
        config.sort();
        assert_eq!(
            config,
            vec!["Debug", "MinSizeRel", "RelWithDebInfo", "Release"]
        );

        let debug_exe_info = &p["Debug"];

        let actual_executable_path =
            crate::common::readable_canonical_path(&debug_exe_info.executable_path)?;
        assert!(actual_executable_path.replace('\\', "/").ends_with(
            "test_data/test_project1/DepRunTest/build-vcxproj-user/DepRunTest/Debug/DepRunTest.exe"
        ));

        assert!(debug_exe_info.debugging_configuration.is_some());
        let deb_config = debug_exe_info.debugging_configuration.as_ref().unwrap();

        assert_eq!(deb_config.configuration, "Debug");

        assert!(deb_config.working_directory.is_some());
        let actual_working_directory = crate::common::readable_canonical_path(
            deb_config.working_directory.as_ref().unwrap(),
        )?;
        assert!(actual_working_directory.replace('\\', "/").ends_with(
            "test_data/test_project1/DepRunTest/build-vcxproj-user/DepRunTestLib/Debug"
        ));

        assert!(deb_config.path.is_some());
        let p = deb_config.path.as_ref().unwrap();
        assert_eq!(p.len(), 1);
        let actual_path = crate::common::readable_canonical_path(p.first().unwrap())?;
        assert!(actual_path.replace('\\', "/").ends_with(
            "test_data/test_project1/DepRunTest/build-vcxproj-user/DepRunTestLib/Debug"
        ));

        Ok(())
    }

    #[test]
    fn vcxproj_no_user() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        let vcxproj_path =
            d.join("test_data/test_project1/DepRunTest/build/DepRunTest/DepRunTest.vcxproj");
        let p = super::parse_vcxproj(&vcxproj_path)?;

        let mut config: Vec<&String> = p.keys().collect();
        config.sort();
        assert_eq!(
            config,
            vec!["Debug", "MinSizeRel", "RelWithDebInfo", "Release"]
        );

        let debug_exe_info = &p["Debug"];

        let actual_executable_path =
            crate::common::readable_canonical_path(&debug_exe_info.executable_path)?;
        assert!(actual_executable_path.replace('\\', "/").ends_with(
            "test_data/test_project1/DepRunTest/build/DepRunTest/Debug/DepRunTest.exe"
        ));

        assert!(debug_exe_info.debugging_configuration.is_none());

        Ok(())
    }

    #[test]
    fn vcxproj_user() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        let vcxproj_path = d.join("test_data/test_project1/DepRunTest/build-vcxproj-user/DepRunTest/DepRunTest.vcxproj.user");
        let p = super::parse_vcxproj_user(&vcxproj_path)?;

        let mut config: Vec<&String> = p.keys().collect();
        config.sort();
        assert_eq!(config, vec!["Debug"]);

        let deb_config = &p["Debug"];

        assert_eq!(deb_config.configuration, "Debug");

        let actual_working_directory = crate::common::readable_canonical_path(
            deb_config.working_directory.as_ref().unwrap(),
        )?;
        assert!(actual_working_directory.replace('\\', "/").ends_with(
            "test_data/test_project1/DepRunTest/build-vcxproj-user/DepRunTestLib/Debug"
        ));

        assert!(deb_config.path.is_some());
        let p = deb_config.path.as_ref().unwrap();
        assert_eq!(p.len(), 1);
        let actual_path = crate::common::readable_canonical_path(p.first().unwrap())?;
        assert!(actual_path.replace('\\', "/").ends_with(
            "test_data/test_project1/DepRunTest/build-vcxproj-user/DepRunTestLib/Debug"
        ));

        Ok(())
    }
}
