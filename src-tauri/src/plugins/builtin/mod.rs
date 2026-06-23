mod image_metadata;

pub use image_metadata::{
    execute as execute_image_metadata, manifest as image_metadata_manifest, MediaGallery,
    MediaItem, MediaType, PLUGIN_ID as IMAGE_METADATA_PLUGIN_ID,
};
