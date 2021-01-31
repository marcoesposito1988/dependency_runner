use crate::common::LookupError;
use crate::system::WindowsSystem;
use crate::vcx::{VcxDebuggingConfiguration, VcxExecutableInformation};
use std::path::{Path, PathBuf};

/// Complete specification of a search task
#[derive(Clone, Debug)]
pub struct LookupQuery {
    /// Path to the root of a Windows installation
    pub system: Option<WindowsSystem>,
    /// Additional executable search path set by the user
    pub user_path: Vec<PathBuf>,
    /// Path to the target executable
    pub target_exe: PathBuf,
    /// Parent directory of target_exe, cached
    pub app_dir: PathBuf,
    /// Working directory as it should appear in the search path
    pub working_dir: PathBuf,
    /// Maximum library recursion depth for the search
    pub max_depth: Option<usize>,
    /// Skip searching dependencies of DLLs found in system directories
    pub skip_system_dlls: bool,
    /// Extract symbols from found DLLs
    pub extract_symbols: bool,
}

impl LookupQuery {
    /// autodetects the settings with sensible defaults
    ///
    /// The working directory will be set to the one containing the executable (i.e. the app_dir)
    #[cfg(windows)]
    pub fn deduce_from_executable_location<P: AsRef<Path>>(
        target_exe: P,
    ) -> Result<Self, LookupError> {
        let app_dir = target_exe
            .as_ref()
            .parent()
            .ok_or(LookupError::ContextDeductionError(
                "Could not find application directory for given executable ".to_owned()
                    + target_exe.as_ref().to_str().unwrap_or("---"),
            ))?;
        Ok(Self {
            system: Some(WindowsSystem::current()?),
            user_path: vec![],
            target_exe: target_exe.as_ref().into(),
            app_dir: app_dir.canonicalize()?,
            working_dir: app_dir.canonicalize()?,
            max_depth: None,
            skip_system_dlls: false,
            extract_symbols: false,
        })
    }

    /// autodetects the settings with sensible defaults
    ///
    /// The working directory will be set to the one containing the executable (i.e. the app_dir)
    #[cfg(not(windows))]
    pub fn deduce_from_executable_location<P: AsRef<Path>>(
        target_exe: P,
    ) -> Result<Self, LookupError> {
        let app_dir = target_exe
            .as_ref()
            .parent()
            .ok_or(LookupError::ContextDeductionError(
                "Could not find application directory for given executable ".to_owned()
                    + target_exe.as_ref().to_str().unwrap_or(""),
            ))?;
        Ok(Self {
            system: WindowsSystem::from_exe_location(&target_exe)?,
            user_path: Vec::new(),
            target_exe: target_exe.as_ref().to_owned(),
            app_dir: app_dir.to_owned(),
            working_dir: app_dir.to_owned(),
            max_depth: None,
            skip_system_dlls: true,
            extract_symbols: false,
        })
    }

    /// update this Query with the information contained in a .vcxproj.user file
    ///
    /// Will set the working directory and the PATH to the ones specified in the file
    pub fn update_from_vcx_debugging_configuration(
        &mut self,
        debugging_configuration: &VcxDebuggingConfiguration,
    ) {
        if let Some(path) = &debugging_configuration.path {
            self.user_path.extend(path.clone());
        }
        if let Some(working_dir) = &debugging_configuration.working_directory {
            self.working_dir = working_dir.clone();
        }
    }

    /// create a Query with the information contained in a .vcxproj file
    ///
    /// Will extract the executable location from the file
    /// If the respective .vcxproj.user file is found, the contained information will be used
    pub fn read_from_vcx_executable_information(
        exe_info: &VcxExecutableInformation,
    ) -> Result<Self, LookupError> {
        let exe_path = std::fs::canonicalize(&exe_info.executable_path)?;

        let app_dir = exe_path.parent().ok_or(LookupError::ContextDeductionError(
            "Could not find application directory for given executable ".to_owned()
                + exe_path.to_str().unwrap_or(""),
        ))?;

        #[cfg(windows)]
        let system = Some(WindowsSystem::current()?);
        #[cfg(not(windows))]
        let system = WindowsSystem::from_exe_location(&exe_path)?;

        let mut ret = Self {
            system,
            user_path: Vec::new(),
            target_exe: exe_path.to_owned(),
            app_dir: app_dir.to_owned(),
            working_dir: app_dir.to_owned(),
            max_depth: None,
            skip_system_dlls: true,
            extract_symbols: false,
        };

        if let Some(debugging_config) = &exe_info.debugging_configuration {
            ret.update_from_vcx_debugging_configuration(debugging_config);
        }

        Ok(ret)
    }
}

#[cfg(test)]
mod tests {
    use crate::query::LookupQuery;
    use crate::LookupError;

    #[test]
    fn build_query() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let relative_path =
            "test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe";
        let exe_path = d.join(relative_path);

        let query = LookupQuery::deduce_from_executable_location(&exe_path)?;
        assert_eq!(query.skip_system_dlls, false);
        assert!(&query.target_exe.ends_with(relative_path));
        assert_eq!(
            &query.working_dir,
            &std::fs::canonicalize(&exe_path.parent().unwrap())?
        );
        assert_eq!(
            &query.app_dir,
            &std::fs::canonicalize(&exe_path.parent().unwrap())?
        );
        assert!(&query.max_depth.is_none());
        #[cfg(windows)]
        {
            use crate::system::WindowsSystem;
            assert_eq!(&query.system, &WindowsSystem::current()?);
        }

        Ok(())
    }
}
