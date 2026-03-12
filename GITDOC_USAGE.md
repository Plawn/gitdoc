# GitDoc Usage Feedback

Journal d'utilisation de GitDoc (le tool MCP) pendant la migration du server Axum vers R2E.

## Points Positifs

- **`ask` très efficace pour comprendre un framework** : En quelques questions, j'ai obtenu une vue complète de R2E (structure des crates, patterns de controllers, AppBuilder API, extractors, etc.) sans avoir à lire manuellement des dizaines de fichiers source.
- **Les réponses `with_source` donnent du code concret** : Pas juste de la doc abstraite, mais des exemples directement utilisables.
- **Le mode conversationnel fonctionne bien** : Les questions de suivi bénéficient du contexte des questions précédentes, ce qui permet d'aller de plus en plus en profondeur.
- **`list_repos` + repo déjà indexé** : Le fait que R2E était déjà enregistré a accéléré le démarrage. L'indexation avec `fetch=true` a pris 142ms pour 651 fichiers.
- **Bonne couverture des sources** : Les réponses citent docs, symboles, et fichiers source — on sait d'où vient l'info.

## Points Négatifs

- **`get_repo_overview` a échoué** avec "error decoding response body" — pas de fallback ni de message d'erreur utile pour diagnostiquer.
- **Les réponses LLM sont parfois trop confiantes** : Sur la question du `Cargo.toml` path pour le git dependency, gitdoc a inventé une réponse plausible mais non vérifiée ("le facade crate est à la racine") plutôt que de dire qu'il ne trouvait pas l'info exacte.
- **Pas de moyen de vérifier le code source directement** en mode simple : les réponses `ask` synthétisent, mais parfois on veut le code brut d'un fichier ou symbole spécifique pour vérifier.
- **Cache Claude Code sur le nom du serveur MCP** : Claude Code cache la liste de tools par nom de serveur MCP. Si on change le nombre de tools (ex: passage simple→granular), le cache n'est pas invalidé tant qu'on garde le même nom. **Solution** : renommer le serveur dans `.mcp.json` (ex: `gitdoc` → `gitdoc2`) pour forcer Claude Code à rafraîchir la liste.
- **`set_mode` dynamique ne fonctionne pas avec Claude Code** : Même si le serveur envoie `notify_tool_list_changed()`, Claude Code ne rafraîchit pas sa liste de tools deferred en cours de session.

## Points d'Attention

- **Toujours croiser les réponses `ask` avec le code réel** quand c'est critique (ex: dépendances Cargo.toml, signatures exactes de fonctions).
- **Indexer avec `fetch=true`** avant de poser des questions pour avoir les données à jour.
- **Le mode `simple` vs `granular`** : En théorie, on peut passer en granular pour accéder aux tools de navigation de code (`get_symbol`, `search_symbols`, etc.). En pratique, Claude Code ne montre pas ces tools malgré `GITDOC_MCP_MODE=granular` dans `.mcp.json`. Workaround : utiliser `ask` avec `detail_level: "with_source"` pour obtenir du code, ou lire directement les fichiers depuis le repo cloné dans `gitdoc_repos/`.
- **Version du binaire gitdoc-mcp** : Penser à `cargo build -p gitdoc-mcp` après toute modification du code MCP, sinon Claude Code utilise l'ancienne version.
