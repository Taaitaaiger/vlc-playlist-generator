use std::io::Write;

use crate::{Playlist, PlaylistNode};

static XML_HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
static PLAYLIST_START_TAG: &str = "<playlist xmlns=\"http://xspf.org/ns/0/\" xmlns:vlc=\"http://www.videolan.org/vlc/playlist/ns/0/\" version=\"1\">";
static PLAYLIST_END_TAG: &str = "</playlist>";
static TITLE_START_TAG: &str = "<title>";
static TITLE_END_TAG: &str = "</title>";
static TRACKLIST_START_TAG: &str = "<trackList>";
static TRACKLIST_END_TAG: &str = "</trackList>";
static TRACK_START_TAG: &str = "<track>";
static TRACK_END_TAG: &str = "</track>";
static LOCATION_START_TAG: &str = "<location>";
static LOCATION_END_TAG: &str = "</location>";
static DURATION_START_TAG: &str = "<duration>";
static DURATION_END_TAG: &str = "</duration>";
static EXTENSION_START_TAG: &str =
    "<extension application=\"http://www.videolan.org/vlc/playlist/0\">";
static EXTENSION_END_TAG: &str = "</extension>";
static VLC_ID_START_TAG: &str = "<vlc:id>";
static VLC_ID_END_TAG: &str = "</vlc:id>";

fn nodes_into_xml<W: Write>(writer: &mut W, nodes: &[PlaylistNode], indent: usize) {
    for node in nodes {
        match node {
            PlaylistNode::File(idx, _) => {
                for _ in 0..indent {
                    write!(writer, "\t").unwrap();
                }
                writeln!(writer, "<vlc:item tid=\"{}\"/>", idx).unwrap();
            }
            PlaylistNode::Dir {
                ref title,
                ref nodes,
            } => {
                for _ in 0..indent {
                    write!(writer, "\t").unwrap();
                }

                writeln!(writer, "<vlc:node title=\"{}\">", title).unwrap();

                nodes_into_xml(writer, nodes, indent + 1);

                for _ in 0..indent {
                    write!(writer, "\t").unwrap();
                }

                writeln!(writer, "</vlc:node>").unwrap();
            }
        }
    }
}

pub(crate) fn into_xml<W: Write>(writer: &mut W, playlist: Playlist) {
    writeln!(writer, "{}", XML_HEADER).unwrap();
    writeln!(writer, "{}", PLAYLIST_START_TAG).unwrap();
    writeln!(
        writer,
        "\t{}Media Library{}",
        TITLE_START_TAG, TITLE_END_TAG
    )
    .unwrap();
    writeln!(writer, "\t{}", TRACKLIST_START_TAG).unwrap();

    for (idx, track) in playlist.tracks().enumerate() {
        writeln!(writer, "\t\t{}", TRACK_START_TAG).unwrap();
        writeln!(
            writer,
            "\t\t\t{}file://{}{}",
            LOCATION_START_TAG,
            url_escape::encode_component(&track.location().to_string_lossy()),
            LOCATION_END_TAG
        )
        .unwrap();

        writeln!(
            writer,
            "\t\t\t{}{}{}",
            TITLE_START_TAG,
            html_escape::encode_text(track.title()),
            TITLE_END_TAG
        )
        .unwrap();

        writeln!(
            writer,
            "\t\t\t{}{}{}",
            DURATION_START_TAG,
            track.duration(),
            DURATION_END_TAG
        )
        .unwrap();

        writeln!(writer, "\t\t\t{}", EXTENSION_START_TAG).unwrap();
        writeln!(
            writer,
            "\t\t\t\t{}{}{}",
            VLC_ID_START_TAG, idx, VLC_ID_END_TAG
        )
        .unwrap();
        writeln!(writer, "\t\t\t{}", EXTENSION_END_TAG).unwrap();

        writeln!(writer, "\t\t{}", TRACK_END_TAG).unwrap();
    }

    writeln!(writer, "\t{}", TRACKLIST_END_TAG).unwrap();

    writeln!(writer, "\t{}", EXTENSION_START_TAG).unwrap();
    nodes_into_xml(writer, playlist.nodes(), 2);
    writeln!(writer, "\t{}", EXTENSION_END_TAG).unwrap();

    writeln!(writer, "{}", PLAYLIST_END_TAG).unwrap();
}
