/*!
Wrapper for PeLite apiset example

Fun with MS Api Set Schemas

Resources:

* https://ofekshilon.com/2016/03/27/on-api-ms-win-xxxxx-dll-and-other-dependency-walker-glitches/
* https://blog.quarkslab.com/runtime-dll-name-resolution-apisetschema-part-i.html
* https://blog.quarkslab.com/runtime-dll-name-resolution-apisetschema-part-ii.html
* https://lucasg.github.io/2017/10/15/Api-set-resolution/
* https://www.geoffchappell.com/studies/windows/win32/apisetschema/index.htm

 */

mod image;
mod win10;

use crate::common::LookupError;
use std::path::Path;
use win10::Entry;

pub type ApisetMap = std::collections::HashMap<String, Vec<String>>;

fn parse_apiset_entry(e: Entry) -> Result<(String, Vec<String>), LookupError> {
    Ok((
        String::from_utf16_lossy(e.name()?).to_lowercase(),
        e.values()?
            .iter()
            .map(|v| String::from_utf16_lossy(v.host_name().unwrap()))
            .collect(),
    ))
}

pub fn parse_apiset<P: AsRef<Path>>(apisetschema_path: P) -> Result<ApisetMap, LookupError> {
    let filemap = pelite::FileMap::open(apisetschema_path.as_ref())?;
    let pefile = pelite::PeFile::from_bytes(&filemap)?;
    if let Some(section) = pefile.section_headers().by_name(".apiset") {
        let apisetschema = win10::Schema::parse(pefile.get_section_bytes(section)?)?;
        let entrymap: Result<ApisetMap, LookupError> = apisetschema
            .entries()?
            .iter()
            .map(parse_apiset_entry)
            .collect();
        entrymap
    } else {
        Ok(ApisetMap::new())
    }
}
