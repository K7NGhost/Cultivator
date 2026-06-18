# Cultivator Plugin Test Data

Use this directory as a datasource when testing Python plugins.

Suggested plugin filters:

```toml
mode = "path_glob"
path_glob = "*/voice/asr/context/phonebook/*.txt"
```

```toml
mode = "path_glob"
path_glob = "*/messages/*.json"
```

Search terms to try with `cultivator_api.search(...)`:

```text
password
token
secret
alice
555
```
