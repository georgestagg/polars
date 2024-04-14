//! # (De)serializing Arrows IPC format.
//!
//! Arrow IPC is a [binary format format](https://arrow.apache.org/docs/python/ipc.html).
//! It is the recommended way to serialize and deserialize Polars DataFrames as this is most true
//! to the data schema.
//!
//! ## Example
//!
//! ```rust
//! use polars_core::prelude::*;
//! use polars_io::prelude::*;
//! use std::io::Cursor;
//!
//!
//! let s0 = Series::new("days", &[0, 1, 2, 3, 4]);
//! let s1 = Series::new("temp", &[22.1, 19.9, 7., 2., 3.]);
//! let mut df = DataFrame::new(vec![s0, s1]).unwrap();
//!
//! // Create an in memory file handler.
//! // Vec<u8>: Read + Write
//! // Cursor<T>: Seek
//!
//! let mut buf: Cursor<Vec<u8>> = Cursor::new(Vec::new());
//!
//! // write to the in memory buffer
//! IpcWriter::new(&mut buf).finish(&mut df).expect("ipc writer");
//!
//! // reset the buffers index after writing to the beginning of the buffer
//! buf.set_position(0);
//!
//! // read the buffer into a DataFrame
//! let df_read = IpcReader::new(buf).finish().unwrap();
//! assert!(df.equals(&df_read));
//! ```
use std::io::{Read, Seek};

use arrow::datatypes::ArrowSchemaRef;
use arrow::io::ipc::read;
use polars_core::frame::ArrowChunk;
use polars_core::prelude::*;

use super::{finish_reader, ArrowReader};
use crate::mmap::MmapBytesReader;
use crate::predicates::PhysicalIoExpr;
use crate::prelude::*;
use crate::RowIndex;

/// Read Arrows IPC format into a DataFrame
///
/// # Example
/// ```
/// use polars_core::prelude::*;
/// use std::fs::File;
/// use polars_io::ipc::IpcReader;
/// use polars_io::SerReader;
///
/// fn example() -> PolarsResult<DataFrame> {
///     let file = File::open("file.ipc").expect("file not found");
///
///     IpcReader::new(file)
///         .finish()
/// }
/// ```
#[must_use]
pub struct IpcReader<R: MmapBytesReader> {
    /// File or Stream object
    pub(super) reader: R,
    /// Aggregates chunks afterwards to a single chunk.
    rechunk: bool,
    pub(super) n_rows: Option<usize>,
    pub(super) projection: Option<Vec<usize>>,
    pub(crate) columns: Option<Vec<String>>,
    pub(super) row_index: Option<RowIndex>,
    memory_map: bool,
    metadata: Option<read::FileMetadata>,
    schema: Option<ArrowSchemaRef>,
}

fn check_mmap_err(err: PolarsError) -> PolarsResult<()> {
    if let PolarsError::ComputeError(s) = &err {
        if s.as_ref() == "memory_map can only be done on uncompressed IPC files" {
            eprintln!(
                "Could not memory_map compressed IPC file, defaulting to normal read. \
                Toggle off 'memory_map' to silence this warning."
            );
            return Ok(());
        }
    }
    Err(err)
}

impl<R: MmapBytesReader> IpcReader<R> {
    fn get_metadata(&mut self) -> PolarsResult<&read::FileMetadata> {
        if self.metadata.is_none() {
            let metadata = read::read_file_metadata(&mut self.reader)?;
            self.schema = Some(metadata.schema.clone());
            self.metadata = Some(metadata);
        }
        Ok(self.metadata.as_ref().unwrap())
    }

    /// Get arrow schema of the Ipc File.
    pub fn schema(&mut self) -> PolarsResult<ArrowSchemaRef> {
        self.get_metadata()?;
        Ok(self.schema.as_ref().unwrap().clone())
    }
    /// Stop reading when `n` rows are read.
    pub fn with_n_rows(mut self, num_rows: Option<usize>) -> Self {
        self.n_rows = num_rows;
        self
    }

    /// Columns to select/ project
    pub fn with_columns(mut self, columns: Option<Vec<String>>) -> Self {
        self.columns = columns;
        self
    }

    /// Add a row index column.
    pub fn with_row_index(mut self, row_index: Option<RowIndex>) -> Self {
        self.row_index = row_index;
        self
    }

    /// Set the reader's column projection. This counts from 0, meaning that
    /// `vec![0, 4]` would select the 1st and 5th column.
    pub fn with_projection(mut self, projection: Option<Vec<usize>>) -> Self {
        self.projection = projection;
        self
    }

    /// Set if the file is to be memory_mapped. Only works with uncompressed files.
    pub fn memory_mapped(mut self, toggle: bool) -> Self {
        self.memory_map = toggle;
        self
    }

    // todo! hoist to lazy crate
    #[cfg(feature = "lazy")]
    pub fn finish_with_scan_ops(
        mut self,
        predicate: Option<Arc<dyn PhysicalIoExpr>>,
        verbose: bool,
    ) -> PolarsResult<DataFrame> {
        if self.memory_map && self.reader.to_file().is_some() {
            if verbose {
                eprintln!("memory map ipc file")
            }
            match self.finish_memmapped(predicate.clone()) {
                Ok(df) => return Ok(df),
                Err(err) => check_mmap_err(err)?,
            }
        }
        let rechunk = self.rechunk;
        let metadata = read::read_file_metadata(&mut self.reader)?;

        // NOTE: For some code paths this already happened. See
        // https://github.com/pola-rs/polars/pull/14984#discussion_r1520125000
        // where this was introduced.
        if let Some(columns) = &self.columns {
            self.projection = Some(columns_to_projection(columns, &metadata.schema)?);
        }

        let schema = if let Some(projection) = &self.projection {
            Arc::new(apply_projection(&metadata.schema, projection))
        } else {
            metadata.schema.clone()
        };

        let reader = read::FileReader::new(self.reader, metadata, self.projection, self.n_rows);

        finish_reader(reader, rechunk, None, predicate, &schema, self.row_index)
    }
}

impl<R: MmapBytesReader> ArrowReader for read::FileReader<R>
where
    R: Read + Seek,
{
    fn next_record_batch(&mut self) -> PolarsResult<Option<ArrowChunk>> {
        self.next().map_or(Ok(None), |v| v.map(Some))
    }
}

impl<R: MmapBytesReader> SerReader<R> for IpcReader<R> {
    fn new(reader: R) -> Self {
        IpcReader {
            reader,
            rechunk: true,
            n_rows: None,
            columns: None,
            projection: None,
            row_index: None,
            memory_map: true,
            metadata: None,
            schema: None,
        }
    }

    fn set_rechunk(mut self, rechunk: bool) -> Self {
        self.rechunk = rechunk;
        self
    }

    fn finish(mut self) -> PolarsResult<DataFrame> {
        if self.memory_map && self.reader.to_file().is_some() {
            match self.finish_memmapped(None) {
                Ok(df) => return Ok(df),
                Err(err) => check_mmap_err(err)?,
            }
        }
        let rechunk = self.rechunk;
        let metadata = read::read_file_metadata(&mut self.reader)?;
        let schema = &metadata.schema;

        if let Some(columns) = &self.columns {
            let prj = columns_to_projection(columns, schema)?;
            self.projection = Some(prj);
        }

        let schema = if let Some(projection) = &self.projection {
            Arc::new(apply_projection(&metadata.schema, projection))
        } else {
            metadata.schema.clone()
        };

        let ipc_reader =
            read::FileReader::new(self.reader, metadata.clone(), self.projection, self.n_rows);
        finish_reader(ipc_reader, rechunk, None, None, &schema, self.row_index)
    }
}
