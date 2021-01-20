use std::collections::HashMap;

use crate::LookupError;
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
    let configuration_condition_text = n.attribute("Condition").ok_or(LookupError::ParseError(
        "Failed to find Condition group".to_owned(),
    ))?;
    let config: String = configuration_re
        .captures_iter(configuration_condition_text)
        .nth(0)
        .ok_or(LookupError::ParseError(
            "Failed to find configuration name".to_owned(),
        ))?
        .get(1)
        .ok_or(LookupError::ParseError(
            "Failed to find configuration name".to_owned(),
        ))?
        .as_str()
        .to_owned();
    Ok(config)
}

fn extract_debugging_configuration_from_config_node(
    n: &roxmltree::Node,
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
        let environment_text = environment_node.text().ok_or(LookupError::ParseError(
            "Failed to find LocalDebuggerEnvironment tag".to_owned(),
        ))?;
        let environment_variables: Vec<&str> = environment_text.lines().collect();
        let path_env_var = environment_variables
            .iter()
            .find(|l| l.trim_start().starts_with("PATH="))
            .ok_or(LookupError::ParseError(
                "Failed to find PATH variable".to_owned(),
            ))?;
        let path_env_var_without_varname =
            path_env_var
                .strip_prefix("PATH=")
                .ok_or(LookupError::ParseError(
                    "Failed to find LocalDebuggerEnvironment tag".to_owned(),
                ))?;
        let path_entries = path_env_var_without_varname.split(";");
        let path_entries_no_vars: Vec<PathBuf> = path_entries
            .filter(|s| !s.contains("$") && !s.is_empty())
            .map(|s| PathBuf::from(s))
            .collect();
        ret.path = Some(path_entries_no_vars);
    }

    if let Some(working_directory_node) = n
        .descendants()
        .find(|n| n.has_tag_name("LocalDebuggerWorkingDirectory"))
    {
        let working_directory_text =
            working_directory_node
                .text()
                .ok_or(LookupError::ParseError(
                    "Failed to find LocalDebuggerEnvironment tag".to_owned(),
                ))?;
        // TODO fetch properties from vcxproj? may get out of hand
        if !working_directory_text.starts_with("$") {
            ret.working_directory = Some(PathBuf::from(working_directory_text));
        }
    }

    Ok(ret)
}

// TODO make private
// extracts the PATH variable from the file (can only relate to a single executable, but there may be specified many configurations)
// and the working directory (<LocalDebuggerWorkingDirectory> property)
pub fn extract_debugging_configuration_per_config_from_vcxproj_user<
    P: AsRef<std::path::Path> + ?Sized,
>(
    p: &P,
) -> anyhow::Result<HashMap<String, VcxDebuggingConfiguration>> {
    let filecontent = std::fs::read_to_string(p)?;
    let doc = roxmltree::Document::parse(&filecontent)?;
    let project_node = doc
        .descendants()
        .find(|n| n.has_tag_name("Project"))
        .ok_or(LookupError::ParseError(
            "Failed to find Project tag".to_owned(),
        ))?;
    let configuration_nodes: Vec<_> = project_node
        .descendants()
        .filter(|n| n.has_tag_name("PropertyGroup"))
        .collect();
    let debugging_config_per_config: HashMap<String, VcxDebuggingConfiguration> =
        configuration_nodes
            .iter()
            .map(|n| extract_debugging_configuration_from_config_node(n))
            .filter_map(Result::ok)
            .map(|e: VcxDebuggingConfiguration| (e.configuration.clone(), e))
            .collect();
    Ok(debugging_config_per_config)
}

/// Executable Information
///
/// Extracted from a .vcxproj file
/// Grouped by configuration (e.g. Debug, Release, ...)
/// Contains VcxDebuggingConfiguration extracted from respective .vcxproj.user, if present
#[derive(Debug)]
pub struct VcxExecutableInformation {
    pub configuration: String,
    pub executable_path: String,
    pub debugging_configuration: Option<VcxDebuggingConfiguration>,
}

pub fn extract_executable_information_per_config_from_vcxproj<
    P: AsRef<std::path::Path> + ?Sized,
>(
    p: &P,
) -> anyhow::Result<HashMap<String, VcxExecutableInformation>> {
    let filecontent = std::fs::read_to_string(p)?;
    let doc = roxmltree::Document::parse(&filecontent)?;
    let project_node = doc
        .descendants()
        .find(|n| n.has_tag_name("Project"))
        .ok_or(LookupError::ParseError(
            "Failed to find Project tag".to_owned(),
        ))?;

    // extract the file path the config refers to (outdir + target name + extension)
    let outdir_per_config: HashMap<String, String> = project_node
        .descendants()
        .filter(|n: &roxmltree::Node| n.has_tag_name("OutDir"))
        .map(|n| {
            if let Some(od) = n.text() {
                extract_config_from_node(&n).and_then(|c| Ok((c.clone(), od.to_owned())))
            } else {
                Err(LookupError::ParseError("Empty OutDir tag".to_owned()))
            }
        })
        .filter_map(Result::ok)
        .collect();
    let targetname_nodes: HashMap<String, String> = project_node
        .descendants()
        .filter(|n| n.has_tag_name("TargetName"))
        .map(|n| {
            if let Some(tn) = n.text() {
                extract_config_from_node(&n).and_then(|c| Ok((c, tn.to_owned())))
            } else {
                Err(LookupError::ParseError("Empty TargetName tag".to_owned()))
            }
        })
        .filter_map(Result::ok)
        .collect();
    let targetext_nodes: HashMap<String, String> = project_node
        .descendants()
        .filter(|n| n.has_tag_name("TargetExt"))
        .map(|n| {
            if let Some(te) = n.text() {
                extract_config_from_node(&n).and_then(|c| Ok((c, te.to_owned())))
            } else {
                Err(LookupError::ParseError("Empty TargetExt tag".to_owned()))
            }
        })
        .filter_map(Result::ok)
        .collect();

    let configs: Vec<_> = outdir_per_config.keys().collect();

    let mut executable_info_per_config: HashMap<String, VcxExecutableInformation> = configs
        .iter()
        .map(|&c| {
            let (e_dir, e_name, e_ext) = (
                &outdir_per_config[c],
                &targetname_nodes[c],
                &targetext_nodes[c],
            );
            // TODO: fix handling of win path from linux
            if let Some(full_path) = std::path::Path::new(e_dir)
                .join(e_name.to_owned() + e_ext)
                .to_str()
            {
                Ok((
                    c.clone(),
                    VcxExecutableInformation {
                        configuration: c.clone(),
                        executable_path: full_path.to_owned(),
                        debugging_configuration: None,
                    },
                ))
            } else {
                Err(LookupError::ParseError(
                    "Could not find executable path".to_owned(),
                ))
            }
        })
        .filter_map(Result::ok)
        .collect();

    if let Some(parent_dir) = p.as_ref().parent() {
        if let Some(filename) = p.as_ref().file_name() {
            let mut vcxuser_filename = filename.to_owned();
            vcxuser_filename.push(".user");
            let vcxproj_user_path = parent_dir.join(vcxuser_filename);
            if vcxproj_user_path.exists() {
                if let Ok(debugging_configuration_per_config) =
                    extract_debugging_configuration_per_config_from_vcxproj_user(&vcxproj_user_path)
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
