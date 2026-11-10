package io.github.demusis.ponte

import org.junit.Rule
import org.junit.Test

class EmailAjustesTest {
    @get:Rule val paparazzi = newPaparazzi()

    private val server = "Conversando com o servidor de e-mail…"

    private fun mail(state: MailState, name: String, dark: Boolean = false) =
        paparazzi.shotInShell(name, 3, dark) { MailScreen(state, {}, {}, {}, {}, {}, {}, {}, {}) }

    @Test fun email() = mail(Samples.mail, "claro")

    @Test fun emailEscuro() = mail(Samples.mail, "escuro", dark = true)

    @Test fun emailSemMarcadas() = mail(Samples.mail.copy(checked = emptySet(), info = "2 mensagens importadas"), "sem_marcadas")

    @Test fun emailListando() = mail(
        Samples.mail.copy(messages = emptyList(), listed = false, checked = emptySet(), busy = true, info = server), "listando"
    )

    @Test fun emailImportando() = mail(Samples.mail.copy(busy = true, info = server), "importando")

    @Test fun emailCaixaVazia() = mail(Samples.mail.copy(messages = emptyList(), checked = emptySet(), filter = "estrela"), "caixa_vazia")

    @Test fun emailErro() = mail(
        Samples.mail.copy(
            messages = emptyList(), listed = false, checked = emptySet(),
            info = "Não foi possível entrar na conta: senha recusada pelo servidor.", infoError = true
        ),
        "erro"
    )

    @Test fun emailPedeSenha() = mail(
        MailState(loading = false, accounts = Samples.accounts, account = Samples.accounts[1], needsPassword = true), "pede_senha"
    )

    @Test fun emailSemConta() = mail(MailState(loading = false), "sem_conta")

    @Test fun emailSemContaEscuro() = mail(MailState(loading = false), "sem_conta_escuro", dark = true)

    private fun settings(state: SettingsState, name: String, dark: Boolean = false) =
        paparazzi.shot(name, dark) { SettingsScreen(state, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) }

    @Test fun ajustes() = settings(Samples.settings, "claro")

    @Test fun ajustesEscuro() = settings(Samples.settings, "escuro", dark = true)

    @Test fun ajustesInteira() {
        paparazzi.tall(3800)
        settings(Samples.settings, "inteira")
    }

    @Test fun ajustesInteiraEscuro() {
        paparazzi.tall(3800)
        settings(Samples.settings, "inteira_escuro", dark = true)
    }

    /** Nothing configured yet, a phone that cannot keep the passphrase, changes waiting. */
    @Test fun ajustesInicial() {
        paparazzi.tall(3800)
        settings(
            SettingsState(
                canRemember = false, version = "0.3.0", pending = true,
                lastStatus = "Sem Wi-Fi: sincronização adiada. Para usar dados móveis, ative em Ajustes."
            ),
            "inicial"
        )
    }

    @Test fun ajustesChaveQuebrada() = settings(
        Samples.settings.copy(
            keyBroken = true, remembered = false,
            lastStatus = "A chave deste aparelho não pôde ser lida.", lastStatusError = true
        ),
        "chave_quebrada"
    )

    private fun account(state: AccountFormState, name: String, dark: Boolean = false) =
        paparazzi.shot(name, dark) { AccountFormScreen(state, {}, {}, {}) }

    @Test fun contaNova() = account(AccountFormState(), "nova")

    @Test fun contaExistente() = account(AccountFormState(Samples.accounts[0]), "existente")

    @Test fun contaExistenteEscuro() = account(AccountFormState(Samples.accounts[0]), "existente_escuro", dark = true)

    @Test fun contaErro() = account(AccountFormState(Samples.accounts[1], busy = false, error = "Informe o servidor e o usuário."), "erro")

    @Test fun contaSalvando() = account(AccountFormState(Samples.accounts[0], busy = true), "salvando")
}
