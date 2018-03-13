//! Provides types and functions for fetching and unpacking a Node installation
//! zip file in Windows operating systems.

use std::io::{self, Read, Seek, SeekFrom, copy};
use std::path::Path;
use std::fs::{File, create_dir_all};

use reqwest;
use progress_read::ProgressRead;
use zip_rs::ZipArchive;
use verbatim::PathExt;

use failure;

define_source_trait! { Source: Read + Seek }

/// A data source for a Node zip archive that has been cached to the filesystem.
pub struct Cached {
    compressed_size: u64,
    source: File
}

impl Cached {

    /// Loads a cached Node zip archive from the specified file.
    pub fn load(source: File) -> io::Result<Cached> {
        let compressed_size = source.metadata()?.len();

        Ok(Cached {
            compressed_size,
            source
        })
    }
}

impl Read for Cached {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.source.read(buf)
    }

}

impl Seek for Cached {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.source.seek(pos)
    }
}

impl Source for Cached {
    fn uncompressed_size(&self) -> Option<u64> {
        None
    }

    fn compressed_size(&self) -> u64 {
        self.compressed_size
    }
}

/// A data source for fetching a Node zip archive from a remote server.
pub struct Remote {
    cached: Cached
}

impl Remote {

    /// Initiate fetching of a Node zip archive from the given URL, returning
    /// a `Remote` data source.
    pub fn fetch(url: &str, cache_file: &Path) -> Result<Remote, failure::Error> {
        let mut response = reqwest::get(url)?;

        if !response.status().is_success() {
            Err(super::HttpError { code: response.status() })?;
        }

        {
            let mut file = File::create(cache_file)?;
            copy(&mut response, &mut file)?;
        }

        Ok(Remote {
            cached: Cached::load(File::open(cache_file)?)?
        })
    }
}

impl Read for Remote {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.cached.read(buf)
    }
}

impl Seek for Remote {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.cached.seek(pos)
    }
}

impl Source for Remote {
    fn uncompressed_size(&self) -> Option<u64> {
        None
    }

    fn compressed_size(&self) -> u64 {
        self.cached.compressed_size
    }
}

/// A Node installation zip archive.
pub struct Archive<S: Source, F: FnMut(&(), usize)> {
    archive: ZipArchive<ProgressRead<S, (), F>>
}

impl<S: Source, F: FnMut(&(), usize)> Archive<S, F> {

    /// Constructs a new `Archive` from the specified data source and with the
    /// specified progress callback.
    pub fn new(source: S, callback: F) -> Result<Archive<S, F>, failure::Error> {
        Ok(Archive {
            archive: ZipArchive::new(ProgressRead::new(source, (), callback))?
        })
    }

}

impl<S: Source, F: FnMut(&(), usize)> Archive<S, F> {

    /// Unpacks the zip archive to the specified destination folder.
    pub fn unpack(self, dest: &Path) -> Result<(), failure::Error> {
        // Use a verbatim path to avoid the legacy Windows 260 byte path limit.
        let dest: &Path = &dest.to_verbatim();

        let mut zip = self.archive;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;

            let (is_dir, subpath) = {
                let name = entry.name();

                // Verbatim paths aren't normalized so we have to use correct r"\" separators.
                (name.ends_with('/'), Path::new(&name.replace('/', r"\")).to_path_buf())
            };

            if is_dir {
                create_dir_all(dest.join(subpath))?;
            } else {
                let mut file = {
                    if let Some(basedir) = subpath.parent() {
                        create_dir_all(dest.join(basedir))?;
                    }
                    File::create(dest.join(subpath))?
                };
                copy(&mut entry, &mut file)?;
            }
        }
        Ok(())
    }

}
