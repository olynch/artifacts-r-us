use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

use axum::http::header;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;

#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
}

pub struct Credential {
    token: String,
}

impl Credential {
    pub fn from_headers(m: &HeaderMap) -> Result<Self, StoreError> {
        match m.get(header::AUTHORIZATION) {
            Some(x) => {
                let val = x
                    .to_str()
                    .map_err(|_| StoreError::Other("bad header encoding".to_string()))?;
                if val.starts_with("Bearer ") {
                    Ok(Credential {
                        token: val[7..].to_owned(),
                    })
                } else {
                    Err(StoreError::Other(
                        "unknown authentication method".to_string(),
                    ))
                }
            }
            None => Err(StoreError::UnprovidedAuthorization),
        }
    }
}

struct Project {
    name: String,
}

impl Project {
    fn new(name: String) -> Result<Self, StoreError> {
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() | ['-', '_'].contains(&c))
        {
            return Err(StoreError::InvalidProject);
        }
        Ok(Project { name })
    }
}

pub struct ProjectReader {
    name: String,
}

impl ProjectReader {
    pub fn name(&self) -> &str {
        &self.name
    }
}

pub struct ProjectWriter {
    name: String,
}

impl ProjectWriter {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn reader(&self) -> &ProjectReader {
        unsafe { &*(self as *const ProjectWriter as *const ProjectReader) }
    }
}

pub struct Version {
    name: String,
}

impl Version {
    pub fn new(name: String) -> Result<Self, StoreError> {
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() | ['-', '_', '.'].contains(&c))
            && name.chars().next() != Some('.')
        {
            return Err(StoreError::InvalidVersion);
        }
        Ok(Version { name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

pub enum StoreError {
    IO(io::Error),
    InvalidProject,
    InvalidVersion,
    InvalidFile,
    CorruptedVersion,
    UnprovidedAuthorization,
    Other(String),
}

impl IntoResponse for StoreError {
    fn into_response(self) -> axum::response::Response {
        use StoreError::*;
        let body = match self {
            IO(e) => e.to_string(),
            InvalidProject => "invalid project name".to_string(),
            InvalidVersion => "invalid version name".to_string(),
            InvalidFile => "invalid file for version".to_string(),
            CorruptedVersion => "corrupted storage for version".to_string(),
            UnprovidedAuthorization => "did not provide authorization".to_string(),
            Other(s) => s,
        };
        (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
    }
}

fn read_dir(dir: &Path) -> Result<Vec<String>, StoreError> {
    let contents = fs::read_dir(dir).map_err(StoreError::IO)?;
    contents
        .map(|e| {
            let e = e?;
            e.file_name()
                .into_string()
                .map_err(|_s| io::Error::new(io::ErrorKind::InvalidData, "couldn't decode utf8"))
        })
        .collect::<Result<_, io::Error>>()
        .map_err(StoreError::IO)
}

fn file_contains<P: AsRef<Path>>(filename: P, line: &str) -> Result<bool, io::Error> {
    let file = fs::File::open(filename)?;
    Ok(io::BufReader::new(file).lines().any(|l| match l {
        Ok(l) => l == line,
        Err(_) => false,
    }))
}

impl Store {
    pub fn new(dir: String) -> Self {
        Store { dir: dir.into() }
    }

    pub fn project_reader(
        &self,
        project_name: String,
        headers: &HeaderMap,
    ) -> Result<ProjectReader, StoreError> {
        let cred = Credential::from_headers(headers)?;
        let project = Project::new(project_name)?;
        self.authorized_reader(&cred, &project)?;
        Ok(ProjectReader { name: project.name })
    }

    pub fn project_writer(
        &self,
        project_name: String,
        headers: &HeaderMap,
    ) -> Result<ProjectWriter, StoreError> {
        let cred = Credential::from_headers(headers)?;
        let project = Project::new(project_name)?;
        self.authorized_writer(&cred, &project)?;
        Ok(ProjectWriter { name: project.name })
    }

    fn authorized_reader(&self, cred: &Credential, project: &Project) -> Result<(), StoreError> {
        let mut reader_list_path = self.dir.clone();
        reader_list_path.push(&project.name);
        reader_list_path.push("readers.txt");
        if file_contains(reader_list_path, &cred.token).map_err(StoreError::IO)? {
            Ok(())
        } else {
            Err(StoreError::Other("unauthorized reader".to_string()))
        }
    }

    fn authorized_writer(&self, cred: &Credential, project: &Project) -> Result<(), StoreError> {
        let mut writer_list_path = self.dir.clone();
        writer_list_path.push(&project.name);
        writer_list_path.push("writers.txt");
        if file_contains(writer_list_path, &cred.token).map_err(StoreError::IO)? {
            Ok(())
        } else {
            Err(StoreError::Other("unauthorized writer".to_string()))
        }
    }

    pub fn list_projects(&self) -> Result<Vec<String>, StoreError> {
        read_dir(&self.dir)
    }

    fn versions_dir(&self, project: &ProjectReader) -> PathBuf {
        let mut versions_dir = self.dir.clone();
        versions_dir.push(&project.name);
        versions_dir.push("versions");
        versions_dir
    }

    pub fn list_versions(&self, project: &ProjectReader) -> Result<Vec<String>, StoreError> {
        read_dir(&self.versions_dir(project))
    }

    pub fn file_for_version(
        &self,
        project: &ProjectReader,
        version: &Version,
    ) -> Result<String, StoreError> {
        let mut version_dir = self.versions_dir(project);
        version_dir.push(&version.name);
        let version_contents = read_dir(&version_dir)?;
        if version_contents.len() != 1 {
            return Err(StoreError::CorruptedVersion);
        }
        Ok(version_contents.into_iter().next().unwrap())
    }

    pub fn path_for_version(
        &self,
        project: &ProjectReader,
        version: &Version,
    ) -> Result<PathBuf, StoreError> {
        let mut path = self.versions_dir(project);
        let file = self.file_for_version(project, version)?;
        path.push(&version.name);
        path.push(&file);
        Ok(path)
    }

    pub fn outpath_for(
        &self,
        project: &ProjectWriter,
        version: &Version,
        file_name: &str,
    ) -> Result<PathBuf, StoreError> {
        let mut version_path = self.versions_dir(project.reader());
        version_path.push(&version.name);
        if fs::exists(&version_path).map_err(StoreError::IO)? {
            if !fs::read_dir(&version_path)
                .map_err(StoreError::IO)?
                .next()
                .is_none()
            {
                return Err(StoreError::Other("version already exists".to_string()));
            }
        }
        fs::create_dir(&version_path).map_err(StoreError::IO)?;
        version_path.push(file_name);
        Ok(version_path)
    }
}
