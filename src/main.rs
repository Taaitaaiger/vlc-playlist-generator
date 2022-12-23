use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fmt::Debug,
    fs::File,
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
};

use clap::Parser;
use matroska::Matroska;
use mp4::Mp4Reader;
use walkdir::{DirEntry, WalkDir};

use crate::xml::into_xml;

mod xml;

struct Track {
    location: PathBuf,
    title: String,
    duration: usize,
}

impl Track {
    fn location(&self) -> &Path {
        &self.location
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn duration(&self) -> usize {
        self.duration
    }
}

struct TrackList {
    tracks: Vec<Track>,
}

#[derive(PartialEq, Eq, Ord)]
enum PlaylistNode {
    Dir {
        title: String,
        nodes: Vec<PlaylistNode>,
    },
    File(usize, OsString),
}

impl PartialOrd for PlaylistNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (PlaylistNode::Dir { .. }, PlaylistNode::File(_, _)) => Some(std::cmp::Ordering::Less),
            (PlaylistNode::File(_, _), PlaylistNode::Dir { .. }) => {
                Some(std::cmp::Ordering::Greater)
            }
            (
                PlaylistNode::Dir { ref title, .. },
                PlaylistNode::Dir {
                    title: ref title2, ..
                },
            ) => title.partial_cmp(title2),
            (PlaylistNode::File(_, p1), PlaylistNode::File(_, p2)) => p1.partial_cmp(p2),
        }
    }
}

impl PlaylistNode {
    fn new(root: &Path, map: &PendingNodeMap) -> Self {
        match map.nodes.get(root).unwrap() {
            PendingNode::Dir { title, node_paths } => {
                let mut nodes = Vec::with_capacity(node_paths.len());
                for path in node_paths {
                    let inner = PlaylistNode::new(path.as_path(), map);
                    nodes.push(inner)
                }

                PlaylistNode::Dir {
                    title: title.clone(),
                    nodes,
                }
            }
            PendingNode::File(idx, s) => PlaylistNode::File(*idx, s.clone()),
        }
    }

    fn sort(&mut self) {
        match self {
            Self::Dir { ref mut nodes, .. } => {
                nodes.sort();
                for n in nodes {
                    n.sort();
                }
            }
            _ => (),
        }
    }
}

struct Playlist {
    track_list: TrackList,
    nodes: Vec<PlaylistNode>,
}

impl Playlist {
    fn tracks(&self) -> impl Iterator<Item = &Track> {
        self.track_list.tracks.iter()
    }

    fn nodes(&self) -> &[PlaylistNode] {
        &self.nodes
    }
}

enum PendingNode {
    Dir {
        title: String,
        node_paths: Vec<PathBuf>,
    },
    File(usize, OsString),
}

struct PendingNodeMap {
    nodes: HashMap<PathBuf, PendingNode>,
    roots: Vec<PathBuf>,
}

impl PendingNodeMap {
    fn new(roots: Vec<PathBuf>) -> Self {
        let mut nodes = HashMap::new();

        for path in roots.iter() {
            let filename = path
                .file_name()
                .unwrap_or(OsStr::new(""))
                .to_str()
                .unwrap_or("?");

            nodes.insert(
                path.clone(),
                PendingNode::Dir {
                    title: filename.into(),
                    node_paths: vec![],
                },
            );
        }

        PendingNodeMap { nodes, roots }
    }

    fn node_for_dir_of(&mut self, path: &Path) -> &mut PendingNode {
        let mut ancestors = path.ancestors().skip(1);
        let parent_path = ancestors.next().unwrap();
        if !self.nodes.contains_key(parent_path) {
            let name = parent_path.file_name().unwrap_or(OsStr::new("?"));
            self.nodes.insert(
                parent_path.to_path_buf(),
                PendingNode::Dir {
                    title: name.to_str().unwrap_or("?").into(),
                    node_paths: vec![],
                },
            );

            self.node_for_dir_of(parent_path);
        }

        let node = self.nodes.get_mut(parent_path).unwrap();
        match node {
            PendingNode::Dir {
                ref mut node_paths, ..
            } => node_paths.push(path.into()),
            _ => unreachable!(),
        }

        node
    }

    fn push_file(&mut self, path: &Path, index: usize) {
        self.node_for_dir_of(path);
        self.nodes.insert(
            path.into(),
            PendingNode::File(index, path.file_name().unwrap().to_os_string()),
        );
    }

    fn into_nodes(self) -> Vec<PlaylistNode> {
        let mut nodes = Vec::with_capacity(self.roots.len());

        for root in &self.roots {
            let node = PlaylistNode::new(root.as_path(), &self);
            nodes.push(node)
        }

        nodes
    }
}

fn mkv_meta<P: AsRef<Path>>(path: P) -> Option<Track> {
    let path = path.as_ref();
    let file = File::open(path).ok()?;
    let mkv = Matroska::open(file).ok()?;

    let duration = mkv.info.duration;
    let title = mkv.info.title;

    let track = Track {
        location: path.to_path_buf(),
        duration: duration.map(|d| d.as_millis() as usize).unwrap_or(0),
        title: title.unwrap_or_else(|| {
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap_or("<No title available>")
                .into()
        }),
    };

    Some(track)
}

fn mp4_meta<P: AsRef<Path>>(path: P) -> Option<Track> {
    let path = path.as_ref();
    let file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let reader = BufReader::new(file);

    let mp4 = Mp4Reader::read_header(reader, size).ok()?;

    let duration = mp4.duration();

    let track = Track {
        location: path.to_path_buf(),
        duration: duration.as_millis() as usize,
        title: path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap_or("<No title available>")
            .into(),
    };

    Some(track)
}

// Track file metadata and directory structure while walking the directory tree.
fn filter<'a>(
    skip: &'a Vec<PathBuf>,
    tracks: &'a mut Vec<Track>,
    nodes: &'a mut PendingNodeMap,
) -> impl 'a + FnMut(&DirEntry) -> bool {
    |entry| {
        let path = entry.path();
        match entry.metadata() {
            Ok(meta) => {
                if meta.is_file() {
                    let file_ext = path.extension();
                    match file_ext {
                        Some(ext) if ext == OsStr::new("mkv") => {
                            if let Some(track) = mkv_meta(path) {
                                nodes.push_file(path, tracks.len());
                                tracks.push(track);
                                return true;
                            }
                        }
                        Some(ext) if ext == OsStr::new("mp4") => {
                            if let Some(track) = mp4_meta(path) {
                                nodes.push_file(path, tracks.len());
                                tracks.push(track);
                                return true;
                            }
                        }
                        _ => return false,
                    }

                    false
                } else if meta.is_dir() {
                    skip.iter().find(|e| e.as_path() == path).is_none()
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(
        short,
        long,
        help = "Starting points for the scanner, glob patterns are not supported
Each root is recursively scanned for mkv and mp4 files"
    )]
    root: Vec<PathBuf>,

    #[arg(
        short,
        long,
        help = "Skipped directories, glob patterns are not supported"
    )]
    skip: Vec<PathBuf>,

    #[arg(
        short,
        long,
        help = "File to write playlist to
if no file is provided the playlist is printed to stdout"
    )]
    output: Option<PathBuf>,
}
fn main() {
    let args = Args::parse();

    let mut tracks = vec![];
    let mut nodes = PendingNodeMap::new(args.root.clone());

    {
        let mut filter = filter(&args.skip, &mut tracks, &mut nodes);

        for root in args.root {
            let walker = WalkDir::new(root).into_iter();
            for _entry in walker.filter_entry(&mut filter) {}
        }
    }

    let track_list = TrackList { tracks };
    let mut nodes = nodes.into_nodes();

    // Sort nodes alphabetically, files after dirs
    nodes.sort();
    for n in nodes.as_mut_slice() {
        n.sort();
    }

    let playlist = Playlist { track_list, nodes };

    if let Some(path) = args.output {
        let file = File::create(path).expect("Playlist file cannot be created");
        let mut writer = BufWriter::new(file);
        into_xml(&mut writer, playlist);
    } else {
        let mut data = vec![];
        into_xml(&mut data, playlist);
        println!(
            "{}",
            String::from_utf8(data).expect("Non-UTF8 encoded data")
        )
    }
}
