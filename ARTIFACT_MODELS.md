# Standard artifact models

The artifact catalog defines 13 main categories and 45 supported model kinds.
`src/features/artifacts/model/artifactCategoryDefinitions.ts` owns category IDs,
display names, model membership, and the primary model for each category.
`artifactModels.ts` exposes the versioned field definitions, indexed by kind and
main category. `types.ts` and `standardArtifacts.ts` define the payload types.

| Main category | Stable category ID | Primary model | Additional model kinds |
| --- | --- | --- | --- |
| Refined Results | `refined_results` | `refined_result` | `timeline_event`, `dictionary_word`, `searched_item`, `carved_string` |
| Web Related | `web_related` | `browser_history` | `visited_page`, `cookie`, `web_bookmark` |
| Communication | `communication` | `message` | `call`, `contact`, `sms`, `mms`, `chat`, `instant_message`, `voice_mail` |
| Social Networking | `social_networking` | `social_profile` | `social_post` |
| Media | `media` | `media` | — |
| Email & Calendar | `email_calendar` | `email` | `calendar_entry` |
| Documents | `documents` | `document` | `file`, `note`, `shared_file` |
| Application Usage | `application_usage` | `application_usage` | `application`, `installed_application` |
| Operating System | `operating_system` | `system` | `account`, `user_account`, `notification`, `powering_event` |
| Encryption & Credentials | `encryption_credentials` | `credential` | `password`, `encrypted_item` |
| Connected Devices | `connected_devices` | `connected_device` | `bluetooth_device`, `wireless_network` |
| Location & Travel | `location_travel` | `location` | `journey`, `map` |
| Custom | `custom` | `record` | `custom_table` |

## Shared contract

Every standard payload has `kind`, `category`, and `label`. Model definitions
declare their required fields and JSON field types. Optional base fields include
`description`, `icon`, `source`, `timestamps`, `tags`, `confidence`, `severity`,
`group`, `deduplication`, and `raw`. Use `source.filePath` for evidence provenance;
include parser and offset information when available. Keep timestamp values in
ISO 8601 form and preserve their original timezone or timestamp provenance.
Missing or unknown values should stay absent rather than becoming zero or an
invented timestamp. File sizes are bytes, durations seconds, distances meters,
and geographic coordinates decimal degrees.

Schema version 1 belongs to the model definition. It does not rewrite existing
case payloads. Payload `category` remains the established API slug, such as
`browser`, `messages`, or `files`; `navigationCategory` on the model definition
provides the main category ID. Known `kind` values determine tree placement.
Unknown kinds, generic records, and custom tables continue to appear under Custom.

## Additional models

| Model | Required fields beyond `label` | Optional fields |
| --- | --- | --- |
| `refined_result` | `finding` | `summary`, `rationale`, `relatedArtifactIds`, `ruleId`, `matchedAt` |
| `social_profile` | `platform`, `username` | `profileId`, `displayName`, `url`, `createdAt`, `lastActiveAt` |
| `social_post` | `platform` | `postId`, `author`, `authorId`, `body`, `url`, `postedAt`, `editedAt`, `attachments`, `replyToId`, `likeCount`, `shareCount` |
| `document` | `path` | `name`, `title`, `author`, `mimeType`, `size`, `sha256`, `pageCount`, `createdAt`, `modifiedAt` |
| `encrypted_item` | `path` | `encryptionType`, `algorithm`, `keyIdentifier`, `provider`, `isLocked` |
| `connected_device` | `name` | `deviceType`, `identifier`, `manufacturer`, `model`, `serialNumber`, `connectionType`, `firstConnectedAt`, `lastConnectedAt` |

`encrypted_item` describes encrypted evidence; credential models describe
credential indicators. Use preview or identifier fields rather than raw secrets.
Attachment and document paths reference evidence rather than embedding file bytes.

## Python plugins

The six additional kinds have matching `cultivator_api` helpers and typed editor
stubs. Existing helpers and custom-table APIs remain available. Helpers fill
`kind` and `category`; plugins supply `label` and the model fields.

```python
import cultivator_api as api

def run(context):
    return api.social_profile(
        "Example profile",
        platform="Example service",
        username="example_user",
        profileId="profile-42",
        source={"filePath": context["file"]["path"], "parser": "profile-parser"},
    )
```

This appears under Social Networking. A plugin returning `document` appears
under Documents, and `connected_device` appears under Connected Devices.
`buildCultivatorApiReferenceForLlms()` includes the category catalog and field
definitions for all 45 models.
