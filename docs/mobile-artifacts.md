# Mobile artifact dependencies

The bundled ALEAPP, iLEAPP and VLEAPP runtimes use managed Python 3.12
environments. Parsers needing separately installed DLLs, external executables or
Git-based dependency chains are excluded. Python packages with self-contained
native wheels (for example Pillow and PyCryptodome) remain supported.

The available catalogs contain 1,296 Android, 1,266 iOS and 211 vehicle artifact
definitions. Removed modules are:

| Platform | Modules | Dependency removed |
| --- | --- | --- |
| Android | Threema, HLD Privacy Safe, WeChat | Native SQLCipher library/DLL |
| Android | Browser Artifacts via Mister Skinnylegs, Tutanota | Git-installed browser/Chromium readers |
| iOS | logarchive and its derived Unified Logs artifacts | External `unifiedlog_iterator` executable |
| Vehicle | None | All bundled parsers use managed Python dependencies |

The iOS native reader downloader and its provisioning code are removed. Unused
SQLCipher, browser-reader and Unified Logs requirements/wheels are removed too.
Android no longer selects Python 3.15 to accommodate the SQLCipher wheel.
Changed bundle/dependency hashes select new installed runtimes and environments;
previous case results and cached environments are not deleted.

`scripts/mobile/runtime_policy.py` defines the exclusions. Vendoring applies this
policy before copying parser files and filters requirements, so refreshing an
upstream checkout cannot restore those modules. Each catalog records exclusions,
original upstream counts, hashes of the filtered runtime files and the original
requirements hash. Source audit verifies unchanged parser bytes and the expected
requirements transformation against the original checkout.

To run the catalog, dependency-policy and protocol tests:

```powershell
python -B -m unittest discover -s tests/python -p test_mobile_artifacts.py -v
cargo test --offline --locked --manifest-path src-tauri/Cargo.toml -p cultivator mobile_artifacts --lib
```

Set `CULTIVATOR_ANDROID_TEST_PYTHON`, `CULTIVATOR_IOS_TEST_PYTHON` and
`CULTIVATOR_VEHICLE_TEST_PYTHON` to isolated interpreters with each filtered
requirements file installed to enable the end-to-end Python tests. The native
on-demand evidence tests additionally run with `-- --ignored`.
