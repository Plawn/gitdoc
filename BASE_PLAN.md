# GitDoc — Architecture & Plan d'implémentation

## Philosophie

**Le LLM ne lit jamais de fichier source brut.** Il navigue le code exclusivement via les symboles extraits par tree-sitter. Les fichiers Markdown sont les seuls contenus lisibles en intégralité.

Cette contrainte élimine le bruit : code mort, boilerplate, imports inutilisés, fichiers de config — tout ce qui ne produit pas de symbole référencé est invisible. Le LLM entre dans un repo par la documentation (le "quoi"), puis descend dans les symboles (le "comment") en suivant un graphe de dépendances.

Deux cas d'usage principaux :

- **Agents support** : comprendre une codebase pour répondre aux questions des utilisateurs. L'agent lit la doc, cherche les concepts, puis plonge dans les symboles pertinents.
- **Agents code** : comprendre le fonctionnement d'une lib ou d'un framework via son repo git. L'agent navigue l'API publique, suit les implémentations, comprend les patterns — potentiellement sur plusieurs versions.

---

## Architecture générale

```
 Claude Code       Claude Code        Autre client
  (agent A)         (agent B)          MCP
     │                  │                │
     ▼                  ▼                ▼
 ┌────────┐        ┌────────┐       ┌────────┐
 │MCP thin│        │MCP thin│       │MCP thin│
 │ client │        │ client │       │ client │
 └───┬────┘        └───┬────┘       └───┬────┘
     │                  │                │
     └──────────────────┼────────────────┘
                        │ HTTP API (REST/JSON)
                        ▼
          ┌──────────────────────────────┐
          │       GitDoc Server          │
          │          (Rust)              │
          │                              │
          │  ┌────────────────────────┐  │
          │  │      HTTP Layer        │  │  axum, routes REST
          │  └───────────┬────────────┘  │
          │              │               │
          │  ┌───────────▼────────────┐  │
          │  │     Query Engine       │  │  Résout les requêtes
          │  └───────────┬────────────┘  │
          │              │               │
          │  ┌───────────▼────────────┐  │
          │  │     Index Layer        │  │
          │  │                        │  │
          │  │  SQLite    Tantivy     │  │
          │  │  Embeddings            │  │
          │  └───────────┬────────────┘  │
          │              │               │
          │  ┌───────────▼────────────┐  │
          │  │      Indexer           │  │
          │  │                        │  │
          │  │  Git Walker            │  │
          │  │  Tree-sitter           │  │
          │  │  Doc Parser            │  │
          │  │  Reference Resolver    │  │
          │  └───────────┬────────────┘  │
          │              │               │
          │  ┌───────────▼────────────┐  │
          │  │     Repo Store         │  │  Accès git (gix)
          │  └────────────────────────┘  │
          └──────────────────────────────┘
```

**Deux binaires distincts :**

- `gitdoc-server` — le serveur core. Tourne en permanence. Gère les repos, l'indexation, les index, l'API HTTP. Un seul processus sert tous les clients.
- `gitdoc-mcp` — le client MCP. Lancé par Claude Code (ou tout client MCP). C'est un traducteur MCP → HTTP. Pas de logique métier, pas de state, pas de stockage.

Cette séparation permet :
- Plusieurs agents partagent les mêmes repos indexés sans duplication.
- L'indexation tourne en arrière-plan indépendamment des sessions MCP.
- Le serveur peut être déployé sur une machine distante si nécessaire.
- Le MCP client reste trivial à maintenir.

---

## Modèle de données

### Concept central : le snapshot par commit

Un même repo peut être indexé à plusieurs commits. Chaque commit produit un **snapshot** indépendant : ses propres docs, ses propres symboles, ses propres références. Ça permet de servir plusieurs versions d'une lib, de comparer des APIs entre tags, ou de naviguer l'historique.

La granularité est le commit (pas la branche). Un tag `v2.0.0` pointe vers un commit, une branche `main` pointe vers un commit — le serveur indexe des commits.

```
repo
 └── snapshot (commit SHA)
      ├── docs
      ├── symbols
      ├── references
      └── embeddings
```

Le stockage est optimisé : deux snapshots du même repo partagent les fichiers inchangés via leurs checksums. Seuls les fichiers modifiés entre deux commits sont ré-extraits.

### Tables SQLite

#### `repos`

| Colonne | Type | Description |
|---------|------|-------------|
| `id` | TEXT PK | Identifiant unique (slug) |
| `path` | TEXT | Chemin local du repo git |
| `name` | TEXT | Nom affiché |
| `created_at` | INTEGER | Timestamp |

#### `snapshots`

Chaque indexation d'un commit produit un snapshot.

| Colonne | Type | Description |
|---------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `repo_id` | TEXT FK | → repos.id |
| `commit_sha` | TEXT | SHA complet du commit |
| `label` | TEXT NULL | Label humain optionnel (`v2.0.0`, `main`, `release-2024-03`) |
| `indexed_at` | INTEGER | Timestamp de l'indexation |
| `status` | TEXT | `indexing`, `ready`, `failed` |
| `stats` | TEXT (JSON) | `{ files_scanned, docs_count, symbols_count, refs_count, duration_ms }` |

Index unique sur `(repo_id, commit_sha)`. Un même commit n'est jamais indexé deux fois.

#### `files`

Table de déduplication. Chaque contenu unique de fichier est stocké une seule fois.

| Colonne | Type | Description |
|---------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `checksum` | TEXT UNIQUE | SHA-256 du contenu |
| `content` | TEXT | Contenu brut (pour les .md) ou NULL (pour les fichiers source, le contenu est dans les symboles) |

#### `snapshot_files`

Lien entre un snapshot et ses fichiers. C'est le "filesystem" d'un snapshot.

| Colonne | Type | Description |
|---------|------|-------------|
| `snapshot_id` | INTEGER FK | → snapshots.id |
| `file_path` | TEXT | Chemin relatif dans le repo |
| `file_id` | INTEGER FK | → files.id |
| `file_type` | TEXT | `doc`, `rust`, `typescript`, `javascript`, `other` |

PK composite sur `(snapshot_id, file_path)`.

#### `docs`

| Colonne | Type | Description |
|---------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `file_id` | INTEGER FK | → files.id |
| `title` | TEXT | Titre extrait (premier `#`) |

Un fichier doc est lié à `files` via son checksum. Si le même `.md` existe dans deux snapshots (contenu identique), il n'est stocké qu'une fois.

#### `symbols`

Table centrale. Chaque symbole extrait par tree-sitter.

| Colonne | Type | Description |
|---------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `file_id` | INTEGER FK | → files.id |
| `name` | TEXT | Nom du symbole |
| `qualified_name` | TEXT | Nom qualifié (`crate::config::parse_config`) |
| `kind` | TEXT | Type (enum ci-dessous) |
| `visibility` | TEXT | `public`, `private`, `protected`, `pub(crate)` |
| `file_path` | TEXT | Chemin relatif (dénormalisé pour les requêtes rapides) |
| `line_start` | INTEGER | Ligne de début |
| `line_end` | INTEGER | Ligne de fin |
| `byte_start` | INTEGER | Offset byte début |
| `byte_end` | INTEGER | Offset byte fin |
| `parent_id` | INTEGER FK NULL | → symbols.id (méthode → struct, etc.) |
| `signature` | TEXT | Signature lisible |
| `doc_comment` | TEXT NULL | Commentaire de doc extrait |
| `body` | TEXT | Code source du symbole |

Index sur `(file_id)`, `(name)`, `(kind)`.

Les symboles sont liés aux `files`, pas directement aux snapshots. Le lien snapshot → symboles passe par `snapshot_files → files → symbols`. Si un fichier n'a pas changé entre deux commits, ses symboles sont réutilisés tels quels.

**Enum `kind`** :

```
function | method | constructor
struct | class | interface | type_alias | enum | enum_variant
trait | impl
constant | static | variable
module | namespace
macro
export | re_export
```

#### `references`

Graphe de dépendances entre symboles.

| Colonne | Type | Description |
|---------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `from_symbol_id` | INTEGER FK | → symbols.id |
| `to_symbol_id` | INTEGER FK | → symbols.id |
| `kind` | TEXT | `calls`, `imports`, `type_ref`, `implements`, `extends`, `field_access` |

Les références sont intra-fichier ou cross-fichier mais toujours **intra-snapshot** (résolues dans le contexte d'un commit donné). Les références cross-fichier sont résolues au moment de l'indexation en utilisant la table `snapshot_files` pour mapper les imports vers les bons fichiers/symboles.

#### `embeddings`

| Colonne | Type | Description |
|---------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `source_type` | TEXT | `doc_chunk` ou `symbol` |
| `source_id` | INTEGER | → docs.id ou symbols.id |
| `text` | TEXT | Texte embeddé |
| `vector` | BLOB | Vecteur sérialisé (f32 × dim) |

Comme les embeddings sont liés aux `docs` et `symbols` qui sont eux-mêmes liés aux `files`, ils sont automatiquement dédupliqués entre snapshots.

### Déduplication — comment ça marche

```
Repo A indexé à v1.0:                    Repo A indexé à v2.0:
  snapshot_files:                          snapshot_files:
    src/auth.rs → file#17 (inchangé)        src/auth.rs → file#17  (même fichier!)
    src/router.rs → file#18                 src/router.rs → file#42 (nouveau contenu)
    README.md → file#19                     README.md → file#43     (mis à jour)
```

`file#17` (et donc tous ses symboles et embeddings) est partagé entre les deux snapshots. Seuls `router.rs` et `README.md` sont ré-indexés.

### Index Tantivy

Les index Tantivy contiennent un champ `file_id` pour pouvoir filtrer par snapshot au moment de la requête (en joignant `snapshot_files`).

**Index `docs`** : champs `file_id`, `title`, `content`.

**Index `symbols`** : champs `file_id`, `name`, `qualified_name`, `kind`, `signature`, `doc_comment`.

---

## API HTTP du serveur

Le serveur expose une API REST JSON. Le MCP client la consomme directement.

### Repos

```
POST   /repos                      Enregistrer un repo local
GET    /repos                      Lister les repos
GET    /repos/:repo_id             Détail d'un repo + ses snapshots
DELETE /repos/:repo_id             Supprimer un repo et ses données
```

### Indexation

```
POST   /repos/:repo_id/index      Déclencher l'indexation d'un commit
  Body: { "commit": "HEAD" | "<sha>" | "<tag>" | "<branch>", "label": "v2.0.0" }
  Retour: { snapshot_id, status: "indexing" }

GET    /repos/:repo_id/snapshots                  Lister les snapshots
GET    /repos/:repo_id/snapshots/:snapshot_id      Détail + stats d'un snapshot
DELETE /repos/:repo_id/snapshots/:snapshot_id      Supprimer un snapshot
```

### Navigation (toutes les routes sont scoped à un snapshot)

```
GET /snapshots/:snapshot_id/overview
  → README, arbo docs, modules top-level

GET /snapshots/:snapshot_id/docs
  → Liste des fichiers markdown
GET /snapshots/:snapshot_id/docs/*path
  → Contenu d'un doc

GET /snapshots/:snapshot_id/symbols
  ?file_path=...&module=...&kind=...&visibility=public&parent_id=...
  → Liste des symboles (sans body)
GET /snapshots/:snapshot_id/symbols/:symbol_id
  → Symbole complet (avec body, children, counts)

GET /snapshots/:snapshot_id/symbols/:symbol_id/references
  ?kind=...&direction=inbound|outbound&limit=20
  → Références entrantes (qui m'utilise) ou sortantes (que j'utilise)

GET /snapshots/:snapshot_id/symbols/:symbol_id/implementations
  → Implémentations (trait↔impl, interface↔class)
```

### Recherche

```
GET /snapshots/:snapshot_id/search/docs
  ?q=...&limit=10
  → Full-text dans la doc (Tantivy)

GET /snapshots/:snapshot_id/search/symbols
  ?q=...&kind=...&visibility=...&limit=10
  → Full-text sur les symboles (Tantivy)

GET /snapshots/:snapshot_id/search/semantic
  ?q=...&scope=all|docs|symbols&limit=10
  → Recherche sémantique (embeddings)
```

### Cross-snapshot (pour la comparaison entre versions)

```
GET /repos/:repo_id/diff/symbols
  ?from_snapshot=...&to_snapshot=...
  → Symboles ajoutés, supprimés, modifiés entre deux snapshots
```

---

## Tools MCP

Le MCP client est un traducteur 1:1 entre les tools MCP et l'API HTTP. Chaque tool correspond à un ou deux appels HTTP.

### Configuration

Le MCP client a besoin d'un seul paramètre de config : l'URL du serveur GitDoc (défaut: `http://localhost:3000`).

### Résolution du snapshot

La plupart des tools prennent un `repo_id` et un `ref` optionnel. Le `ref` peut être :

- Absent → utilise le snapshot le plus récent du repo
- Un label → `v2.0.0`, `main`
- Un SHA (complet ou préfixe)

Le MCP client résout le `ref` en `snapshot_id` via `GET /repos/:repo_id/snapshots` puis utilise le `snapshot_id` pour les appels suivants. Il peut cacher cette résolution pendant la session.

### Tools exposés

#### Exploration

**`list_repos`** — Liste les repos indexés.
```
Params:  (aucun)
Retour:  [{ id, name, path, snapshot_count, latest_snapshot: { label, commit_sha, indexed_at } }]
```

**`get_repo_overview`** — Point d'entrée pour découvrir un repo à un commit donné. Retourne le README, l'arborescence des docs, et les modules de premier niveau.
```
Params:  repo_id, ref?
Retour:  {
  snapshot: { commit_sha, label },
  readme: string | null,
  doc_tree: [{ path, title }],
  modules: [{ name, public_symbol_count, kinds_summary }]
}
```

**`index_repo`** — Déclenche l'indexation d'un commit.
```
Params:  repo_id | path, commit?, label?
Retour:  { snapshot_id, status }
```

#### Documentation

**`list_docs`** — Liste les fichiers markdown.
```
Params:  repo_id, ref?, path_prefix?
Retour:  [{ path, title }]
```

**`read_doc`** — Lit le contenu intégral d'un fichier markdown. Seul tool qui retourne du contenu de fichier brut.
```
Params:  repo_id, ref?, path
Retour:  { path, title, content }
```

#### Symboles

**`list_symbols`** — Liste les symboles d'un scope. Par défaut : publics, premier niveau.
```
Params:  repo_id, ref?, file_path?, module?, kind?, include_private? (défaut: false)
Retour:  [{
  id, name, qualified_name, kind, visibility, signature,
  doc_comment, children_count, file_path, line_start
}]
```

**`get_symbol`** — Récupère un symbole complet avec son code source.
```
Params:  symbol_id | (repo_id, ref?, qualified_name)
Retour:  {
  id, name, qualified_name, kind, visibility, signature,
  file_path, line_start, line_end,
  doc_comment, body,
  children: [{ id, name, kind, signature, doc_comment }],
  referenced_by_count, references_count
}
```

**`find_references`** — Qui utilise ce symbole ?
```
Params:  symbol_id, kind?, limit? (défaut: 20)
Retour:  [{
  symbol: { id, name, qualified_name, kind, signature, file_path },
  ref_kind
}]
```

**`get_dependencies`** — Que utilise ce symbole ?
```
Params:  symbol_id, kind?, limit? (défaut: 20)
Retour:  [{
  symbol: { id, name, qualified_name, kind, signature, file_path },
  ref_kind
}]
```

**`get_implementations`** — Trait↔impl, interface↔class.
```
Params:  symbol_id
Retour:  [{
  symbol: { id, name, qualified_name, kind, signature, file_path },
  direction: "implements" | "implemented_by"
}]
```

#### Recherche

**`search_docs`** — Full-text dans la documentation.
```
Params:  repo_id, ref?, query, limit? (défaut: 10)
Retour:  [{ path, title, snippets: [string] }]
```

**`search_symbols`** — Full-text sur les symboles.
```
Params:  repo_id, ref?, query, kind?, visibility?, limit? (défaut: 10)
Retour:  [{ id, name, qualified_name, kind, visibility, signature, doc_comment, file_path }]
```

**`semantic_search`** — Recherche par sens (embeddings).
```
Params:  repo_id, ref?, query, scope? ("docs"|"symbols"|"all"), limit? (défaut: 10)
Retour:  [{
  source_type: "doc" | "symbol",
  score,
  doc?: { path, title, snippet },
  symbol?: { id, name, qualified_name, kind, signature, doc_comment }
}]
```

#### Cross-version

**`diff_symbols`** — Compare les symboles publics entre deux snapshots.
```
Params:  repo_id, from_ref, to_ref
Retour:  {
  added: [{ name, qualified_name, kind, signature }],
  removed: [{ name, qualified_name, kind, signature }],
  modified: [{
    name, qualified_name, kind,
    old_signature, new_signature,
    signature_changed: bool
  }]
}
```

---

## Pipeline d'indexation

### Étape 1 — Résolution du commit

Recevoir la requête (`repo_id`, `commit`). Résoudre `commit` vers un SHA complet via gix :
- `HEAD` → SHA courant
- `main`, `v2.0.0` → résolution de ref/tag
- SHA partiel → complétion

Vérifier que le `(repo_id, commit_sha)` n'existe pas déjà dans `snapshots`. Si oui, retourner le snapshot existant.

Créer un snapshot avec `status: "indexing"`.

### Étape 2 — Git Walk & déduplication

Lister tous les fichiers trackés à ce commit via `gix`. Pour chaque fichier :

1. Calculer le SHA-256 du contenu.
2. Chercher dans `files` par `checksum`.
3. Si trouvé → réutiliser le `file_id` existant (pas de re-parsing).
4. Si nouveau → insérer dans `files`, marquer pour parsing.
5. Insérer dans `snapshot_files`.

Classer chaque fichier nouveau par type :
- `.md` / `.mdx` → Doc Parser
- `.rs` → Tree-sitter Rust
- `.ts` / `.tsx` / `.js` / `.jsx` → Tree-sitter TypeScript/JS
- Tout le reste → ignoré

### Étape 3 — Doc Parser

Pour chaque nouveau fichier markdown :

- Extraire le titre (premier `# heading`)
- Insérer dans `docs`
- Indexer dans Tantivy (full-text)
- Découper en chunks (~500 tokens, respecter les sections `##`) pour les embeddings

### Étape 4 — Tree-sitter Parse

Pour chaque nouveau fichier source, parser l'AST et extraire les symboles.

**Queries tree-sitter pour Rust** :

```scheme
;; Fonctions
(function_item
  name: (identifier) @name
  parameters: (parameters) @params
  return_type: (_)? @return_type
  body: (block) @body) @function

;; Structs
(struct_item
  name: (type_identifier) @name
  body: (field_declaration_list)? @body) @struct

;; Enums
(enum_item
  name: (type_identifier) @name
  body: (enum_variant_list) @body) @enum

;; Traits
(trait_item
  name: (type_identifier) @name
  body: (declaration_list) @body) @trait

;; Impl blocks
(impl_item
  trait: (type_identifier)? @trait_name
  type: (type_identifier) @type_name
  body: (declaration_list) @body) @impl

;; Type aliases
(type_item
  name: (type_identifier) @name
  type: (_) @type) @type_alias

;; Constants
(const_item
  name: (identifier) @name
  type: (_) @type
  value: (_) @value) @const

;; Modules
(mod_item
  name: (identifier) @name) @module

;; Macros
(macro_definition
  name: (identifier) @name) @macro
```

**Queries tree-sitter pour TypeScript/JavaScript** :

```scheme
;; Functions
(function_declaration
  name: (identifier) @name
  parameters: (formal_parameters) @params
  return_type: (type_annotation)? @return_type
  body: (statement_block) @body) @function

;; Classes
(class_declaration
  name: (type_identifier) @name
  body: (class_body) @body) @class

;; Interfaces
(interface_declaration
  name: (type_identifier) @name
  body: (interface_body) @body) @interface

;; Type aliases
(type_alias_declaration
  name: (type_identifier) @name
  value: (_) @type) @type_alias

;; Enums
(enum_declaration
  name: (identifier) @name
  body: (enum_body) @body) @enum

;; Exported declarations
(export_statement
  declaration: (_) @decl) @export
```

Pour chaque symbole capturé, extraire :

- **Nom** : nœud `@name`
- **Visibilité** : présence de `pub`/`export`, `visibility_modifier` dans l'AST
- **Signature** : texte du nœud parent sans le body (tout ce qui précède `@body`)
- **Doc comment** : commentaires `///`, `/** */` immédiatement avant le nœud
- **Body** : texte complet du nœud (byte_start → byte_end)
- **Parent** : si le nœud est dans un `impl_item` ou `class_body`, lien vers le symbole parent
- **Qualified name** : reconstruit depuis le chemin du fichier + module + nom

### Étape 5 — Résolution des références

Pour chaque symbole du snapshot, analyser son body pour identifier les références vers d'autres symboles.

**Phase A — Extraction des imports** : parser les `use` (Rust) et `import` (TS/JS) de chaque fichier. Construire une table de résolution `nom_local → qualified_name`.

**Phase B — Scan des bodies** : pour chaque identifiant dans le body d'un symbole :
1. Chercher dans la table d'imports du fichier
2. Chercher dans les symboles du même fichier
3. Chercher dans les symboles publics du même module
4. Si trouvé → insérer dans `references`

**Phase C — Relations structurelles** : extraire automatiquement les relations `implements`/`extends` depuis les `impl Trait for Struct` (Rust) et `class X extends Y implements Z` (TS).

La résolution est best-effort. Les faux positifs sont acceptables — le LLM peut évaluer. Les faux négatifs sont inévitables sans analyse sémantique complète.

### Étape 6 — Embeddings

Générer des embeddings pour :

- Chaque chunk de doc (markdown découpé en sections)
- Chaque symbole : texte = `{kind} {name}: {signature}\n{doc_comment}`

Le provider d'embedding est un trait configurable :

```rust
#[async_trait]
trait EmbeddingProvider: Send + Sync {
    fn dimensions(&self) -> usize;
    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
}
```

Implémentations prévues : API Voyage (`voyage-code-3`), API OpenAI, endpoint HTTP custom.

### Étape 7 — Finalisation

Mettre à jour le snapshot : `status: "ready"`, calculer les stats, indexer dans Tantivy.

---

## Structure des deux projets

### `gitdoc-server/`

```
gitdoc-server/
├── Cargo.toml
├── src/
│   ├── main.rs                   Point d'entrée, config, lance axum
│   ├── config.rs                 Configuration (port, DB path, embedding provider)
│   ├── api/
│   │   ├── mod.rs
│   │   ├── repos.rs              Routes /repos
│   │   ├── snapshots.rs          Routes /snapshots
│   │   ├── docs.rs               Routes docs
│   │   ├── symbols.rs            Routes symboles + references
│   │   ├── search.rs             Routes recherche
│   │   └── diff.rs               Route diff cross-snapshot
│   ├── index/
│   │   ├── mod.rs
│   │   ├── db.rs                 SQLite (rusqlite) — schema, migrations, queries
│   │   ├── tantivy.rs            Index full-text
│   │   └── embeddings.rs         Stockage + cosine similarity
│   ├── indexer/
│   │   ├── mod.rs
│   │   ├── pipeline.rs           Orchestration du pipeline
│   │   ├── git_walker.rs         Parcours fichiers via gix
│   │   ├── doc_parser.rs         Parsing markdown
│   │   ├── ts_parser.rs          Extraction symboles tree-sitter
│   │   ├── reference_resolver.rs Résolution des références
│   │   └── languages/
│   │       ├── mod.rs
│   │       ├── rust.rs           Queries + extraction spécifiques Rust
│   │       └── typescript.rs     Queries + extraction spécifiques TS/JS
│   └── embeddings/
│       ├── mod.rs                Trait EmbeddingProvider
│       ├── voyage.rs             Implémentation Voyage AI
│       └── openai.rs             Implémentation OpenAI
```

### `gitdoc-mcp/`

```
gitdoc-mcp/
├── Cargo.toml
├── src/
│   ├── main.rs                   Point d'entrée, lance le serveur MCP
│   ├── config.rs                 URL du serveur GitDoc
│   ├── client.rs                 Client HTTP vers gitdoc-server (reqwest)
│   ├── tools.rs                  Définition des tools MCP → mapping vers client.rs
│   └── snapshot_resolver.rs      Cache ref → snapshot_id
```

Le MCP client est volontairement minimal. Toute la logique est dans le serveur.

### Dépendances principales

**gitdoc-server — Cargo.toml** :

```toml
[dependencies]
# HTTP
axum = "0.8"
tower-http = { version = "0.6", features = ["cors"] }
tokio = { version = "1", features = ["full"] }

# Git
gix = { version = "0.68", features = ["blocking-network-client"] }

# Tree-sitter
tree-sitter = "0.24"
tree-sitter-rust = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-javascript = "0.23"

# Storage
rusqlite = { version = "0.32", features = ["bundled"] }
tantivy = "0.22"

# Embeddings
reqwest = { version = "0.12", features = ["json"] }

# Serde
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Utils
anyhow = "1"
sha2 = "0.10"
tracing = "0.1"
tracing-subscriber = "0.3"
```

**gitdoc-mcp — Cargo.toml** :

```toml
[dependencies]
rmcp = { version = "0.1", features = ["server", "transport-stdio", "transport-streamable-http"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
tracing = "0.1"
```

---

## Flows de navigation type

### Flow 1 — Agent support : "Comment fonctionne l'authentification ?"

```
1. list_repos()
   → [{ id: "my-api", latest_snapshot: { label: "main", ... } }]

2. get_repo_overview(repo_id: "my-api")
   → README + doc_tree contient "docs/authentication.md"

3. read_doc(repo_id: "my-api", path: "docs/authentication.md")
   → Contenu complet du guide d'auth

4. search_symbols(repo_id: "my-api", query: "auth", kind: "function")
   → authenticate(), verify_token(), create_session()

5. get_symbol(symbol_id: 42)      // authenticate()
   → Signature + doc + body

6. get_dependencies(symbol_id: 42)
   → authenticate() appelle verify_token(), query_user_db()

7. get_symbol(symbol_id: 45)      // verify_token()
   → L'agent a assez de contexte pour répondre
```

### Flow 2 — Agent code : "Quoi de neuf dans la v3 du framework ?"

```
1. get_repo_overview(repo_id: "cool-framework", ref: "v3.0.0")
   → README v3, modules

2. diff_symbols(repo_id: "cool-framework", from_ref: "v2.0.0", to_ref: "v3.0.0")
   → added: [Router.middleware(), StreamResponse]
   → removed: [LegacyHandler]
   → modified: [Router.add_route (signature changée)]

3. get_symbol(repo_id: "cool-framework", ref: "v3.0.0",
              qualified_name: "src/router.rs::Router::middleware")
   → Nouvelle méthode, son implémentation

4. L'agent comprend les changements d'API et peut adapter le code
```

### Flow 3 — Recherche sémantique : "Où est gérée la pagination ?"

```
1. semantic_search(repo_id: "my-api", query: "pagination des résultats")
   → doc: docs/api-guidelines.md (section pagination)
   → symbol: paginate() in src/utils/pagination.rs
   → symbol: PaginatedResponse in src/models/response.rs

2. read_doc(path: "docs/api-guidelines.md")
   → Section pagination

3. get_symbol(qualified_name: "src/utils/pagination.rs::paginate")
   → Implémentation
```

---

## Plan d'implémentation

### Phase 0 — Bootstrap des deux projets

**Objectif** : les deux binaires compilent, le MCP se connecte au serveur, un tool `ping` fonctionne end-to-end.

Tâches :
1. Workspace Cargo avec `gitdoc-server` et `gitdoc-mcp`
2. Serveur : axum minimal, route `GET /health` → 200
3. MCP : serveur MCP minimal avec tool `ping` → appelle `GET /health` → retourne "pong"
4. Tester end-to-end : Claude Code → MCP → HTTP → serveur → "pong"

**Critère de succès** : le round-trip complet fonctionne.

### Phase 1 — Schema & indexation basique

**Objectif** : indexer un repo local à un commit, stocker docs et symboles dans SQLite.

Tâches :
1. **Schema SQLite** : toutes les tables décrites ci-dessus, migrations rusqlite
2. **Git Walker** : gix, lister les fichiers à un commit donné, checksums, déduplication via `files`
3. **Doc Parser** : lire les `.md`, extraire titre, stocker
4. **Tree-sitter Rust** : queries pour tous les types de symboles
5. **Tree-sitter TypeScript** : idem TS/JS
6. **Pipeline** : orchestration git walk → classify → parse → stockage
7. **Route API** : `POST /repos` + `POST /repos/:id/index`
8. **Test multi-commit** : indexer deux commits du même repo, vérifier la déduplication des fichiers inchangés

**Critère de succès** : indexer `gitdoc-server` lui-même à HEAD et à un commit antérieur. Les fichiers inchangés partagent le même `file_id`.

### Phase 2 — Tools MCP de navigation

**Objectif** : un agent peut découvrir un repo, lire sa doc, naviguer ses symboles.

Tâches :
1. **Routes API** : `/repos`, `/snapshots/:id/overview`, docs, symbols
2. **Tools MCP** : `list_repos`, `get_repo_overview`, `index_repo`, `list_docs`, `read_doc`, `list_symbols`, `get_symbol`
3. **Snapshot resolver** dans le MCP : résolution `ref` → `snapshot_id`
4. Tester avec Claude Code

**Critère de succès** : un agent fait le flow 1 complet.

### Phase 3 — Graphe de références

**Objectif** : les symboles sont liés entre eux.

Tâches :
1. **Import resolver** : parser `use`/`import`, table de résolution
2. **Body scanner** : identifier les identifiants référencés
3. **Relations structurelles** : `impl Trait for Struct`, `extends`, `implements`
4. **Routes API** : references avec direction inbound/outbound, implementations
5. **Tools MCP** : `find_references`, `get_dependencies`, `get_implementations`

**Critère de succès** : navigation par graphe fonctionnelle.

### Phase 4 — Full-text search (Tantivy)

**Objectif** : recherche rapide dans les docs et symboles.

Tâches :
1. **Index Tantivy docs** : contenu markdown avec `file_id`
2. **Index Tantivy symbols** : nom, signature, doc_comment avec `file_id`
3. **Filtrage par snapshot** : jointure avec `snapshot_files` au query time
4. **Routes API** + **Tools MCP** : `search_docs`, `search_symbols`

**Critère de succès** : `search_symbols("auth")` retourne les résultats pertinents du bon snapshot.

### Phase 5 — Recherche sémantique

**Objectif** : recherche par sens.

Tâches :
1. **Trait EmbeddingProvider** + implémentation Voyage/OpenAI
2. **Chunking docs** : découpe markdown en sections
3. **Embedding symboles** : `{kind} {name}: {signature}\n{doc_comment}`
4. **Stockage BLOB** + **cosine similarity** brute-force filtré par snapshot
5. **Route API** + **Tool MCP** : `semantic_search`

**Critère de succès** : "comment sont gérés les droits" retourne des résultats pertinents.

### Phase 6 — Diff cross-version

**Objectif** : comparer les API entre commits.

Tâches :
1. **Logique de diff** : comparer symboles publics par `qualified_name` entre deux snapshots
2. **Route API** + **Tool MCP** : `diff_symbols`

**Critère de succès** : `diff_symbols("v1.0", "v2.0")` détecte les changements d'API.

### Phase 7 — Polish & robustesse

Tâches :
1. Gestion d'erreurs avec messages actionnables
2. Config TOML (port, DB path, embedding provider, exclusion patterns)
3. Patterns d'exclusion par défaut (`node_modules/`, `target/`, `.git/`, `vendor/`)
4. Tracing structuré
5. Tests d'intégration avec fixtures
6. Garbage collection des `files` orphelins et snapshots supprimés

---

## Décisions techniques ouvertes

**Résolution de références cross-fichiers** : `use crate::...` Rust = fiable. `import from './module'` TS = résolvable. Cas difficiles : re-exports, barrel files, glob imports. Approche : best-effort, on itère.

**Modèle d'embedding** : agnostique via le trait. Recommandation : `voyage-code-3` (spécialisé code) ou `text-embedding-3-small` (OpenAI, moins cher).

**Taille des bodies** : body complet par défaut. `max_lines` optionnel, pas prioritaire.

**Indexation async** : synchrone en V1 (POST bloquant). Async optionnel en phase 7 (retour immédiat, polling du status).

**Repos distants** : V1 = repos locaux. Clone automatique = futur. Le serveur pourrait maintenir un cache de bare repos.

**Granularité multi-commit** : indexation de commits explicites uniquement. Pas de suivi automatique de branches.