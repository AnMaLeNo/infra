// Ouvre dans l'extension Claude Code la session demandée par le front de l'infra (id déposé dans runs/.editor-open).
const vscode = require("vscode"), fs = require("fs"), path = require("path");

exports.activate = () => {
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
  // Après la restauration de l'interface : plus tôt, la commande est sans effet.
  if (/^[0-9a-f-]{36}$/i.test(id)) setTimeout(() => vscode.commands.executeCommand("claude-vscode.primaryEditor.open", id), 4000);
};
