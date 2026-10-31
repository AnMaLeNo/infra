// Ouvre dans l'extension Claude Code la session demandée par le front de l'infra (id déposé dans runs/.editor-open).
const vscode = require("vscode"), fs = require("fs"), path = require("path");

const CLAUDE = "Anthropic.claude-code", OPEN = "claude-vscode.primaryEditor.open";

const claudeTabOpen = () =>
  vscode.window.tabGroups.all.some((g) => g.tabs.some((t) => t.input && /claude/i.test(String(t.input.viewType ?? "")) && t.isActive));

exports.activate = async () => {
  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (!root) return;
  const file = path.join(root, "runs", ".editor-open");
  let id;
  try {
    const fresh = Date.now() - fs.statSync(file).mtimeMs < 120000;
    id = fs.readFileSync(file, "utf8").trim();
    fs.unlinkSync(file);
    if (!fresh) return;
  } catch { return; }
  if (!/^[0-9a-f-]{36}$/i.test(id)) return;
  // Pas de délai fixe : on active l'extension Claude Code nous-mêmes (sinon la commande n'existe pas encore), puis on
  // envoie la commande et on vérifie qu'un onglet Claude est bien actif, en réessayant toutes les 0,5 s pendant 15 s.
  try { await vscode.extensions.getExtension(CLAUDE)?.activate(); } catch { /* l'extension dira elle-même ce qui manque */ }
  for (let i = 0; i < 30; i++) {
    try { await vscode.commands.executeCommand(OPEN, id); } catch { /* commande pas encore enregistrée */ }
    await new Promise((r) => setTimeout(r, 500));
    if (claudeTabOpen()) return;
  }
};
