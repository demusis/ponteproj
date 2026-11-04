"use strict";
const check = document.getElementById("check");
const status = document.getElementById("status");
const hint = document.getElementById("hint");
const help = document.getElementById("help");
const extensionId = document.getElementById("extension-id");
const copyId = document.getElementById("copy-id");
extensionId.textContent = chrome.runtime.id;
copyId.addEventListener("click", () => {
  navigator.clipboard?.writeText(chrome.runtime.id).then(() => { copyId.textContent = "copiado"; setTimeout(() => { copyId.textContent = "copiar"; }, 1500); }).catch(() => {});
});
function show(ok, text, extra) {
  status.textContent = text;
  status.className = `status ${ok ? "ok" : "bad"}`;
  hint.textContent = extra || "";
  help.open = !ok;
}
function verify() {
  check.disabled = true;
  status.textContent = "Verificando a ligação com o Ponte…";
  status.className = "status";
  chrome.runtime.sendMessage({ action: "ponte.ping" }, response => {
    check.disabled = false;
    if (chrome.runtime.lastError) show(false, "✗ A extensão não respondeu.", "Feche e abra o navegador. Se continuar, remova e carregue a extensão de novo.");
    else if (response?.ok === true && response.protocol_version === 1) show(true, `✓ Ligado ao Ponte${response.app_version ? " " + response.app_version : ""}.`, "Abra um documento no SIGADOC e clique em “Importar para o assistente”.");
    else show(false, "✗ Não foi possível ligar ao Ponte.", response?.error?.message || "Tente de novo em alguns segundos.");
  });
}
check.addEventListener("click", verify);
verify();
