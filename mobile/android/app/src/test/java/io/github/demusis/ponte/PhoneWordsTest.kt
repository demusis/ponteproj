package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import java.io.FileNotFoundException
import java.io.IOException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException

/** The words of the phone: what the core says for the computer, and the failures of the system. */
class PhoneWordsTest {
    private fun adapt(s: String) = PhoneWords.adapt(s)

    private val work = "/data/user/0/io.github.demusis.ponte/no_backup/sync-work-1731200000000.ponte"

    @Test fun senhaIncorreta() = assertEquals(
        "Senha de sincronização incorreta, ou o arquivo está danificado ou ainda sendo gravado pelo Drive. " +
            "Confira a senha e espere o Drive terminar de sincronizar.",
        adapt(
            "Senha de sincronização incorreta, ou arquivo danificado ou ainda sendo gravado pela nuvem. " +
                "Confira a senha e espere a pasta terminar de sincronizar."
        )
    )

    @Test fun versaoMaisNovaNoCabecalho() = assertEquals(
        "O arquivo de sincronização foi gravado por uma versão mais nova do Ponte. Atualize o Ponte de bolso.",
        adapt("O arquivo de sincronização foi gravado por uma versão mais nova do Ponte. Atualize o Ponte.")
    )

    @Test fun versaoMaisNovaDaCopia() = assertEquals(
        "O arquivo de sincronização foi gravado por uma versão mais nova do Ponte. Atualize o Ponte de bolso.",
        adapt("A cópia em $work foi gravada por um Ponte mais novo; atualize o Ponte neste computador.")
    )

    @Test fun pastaSemAUltimaVersao() = assertEquals(
        "O Drive ainda não entregou a versão mais recente do arquivo de sincronização. " +
            "Espere o Drive terminar de sincronizar e tente de novo.",
        adapt(
            "A pasta ainda não recebeu a versão mais recente (revisão 12 na pasta, 14 neste computador). " +
                "Espere a nuvem terminar de sincronizar e tente de novo."
        )
    )

    @Test fun outroAcervo() = assertEquals(
        "O arquivo de sincronização é de outro acervo, não o deste aparelho. " +
            "Em Ajustes, toque em Trocar arquivo e escolha o arquivo certo.",
        adapt("A pasta $work contém outro acervo, não o deste computador. Retire-a da lista ou escolha outro arquivo.")
    )

    @Test fun arquivoNaoEncontrado() = assertEquals(
        "O arquivo de sincronização não foi encontrado no Drive. Em Ajustes, toque em Trocar arquivo e escolha-o de novo.",
        adapt("O arquivo de sincronização não foi encontrado.")
    )

    @Test fun falhaInterna() = assertEquals(PhoneWords.INTERNAL, adapt("Falha interna no núcleo do Ponte."))

    @Test fun fraseSenhaVaziaViraSenhaDeSincronizacao() = assertEquals(
        "Digite a senha de sincronização.", adapt("Digite a frase-senha da sincronização.")
    )

    @Test fun naoEArquivoDeSincronizacao() = assertEquals(
        "O arquivo escolhido não é um arquivo de sincronização do Ponte. " +
            "Em Ajustes, toque em Trocar arquivo e escolha o arquivo certo.",
        adapt("$work não é um arquivo de sincronização do Ponte.")
    )

    @Test fun naoFoiPossivelLerEGravar() {
        assertEquals("Não foi possível ler o arquivo de sincronização.", adapt("Não foi possível ler $work."))
        assertEquals("Não foi possível gravar o arquivo de sincronização.", adapt("Não foi possível gravar $work."))
    }

    @Test fun errosDoSistemaOperacionalSaem() {
        assertEquals(
            "Não foi possível ler o arquivo de sincronização.",
            adapt("Não foi possível ler $work.: Permission denied (os error 13)")
        )
        assertEquals(PhoneWords.NO_SPACE, adapt("Não foi possível gravar $work.: No space left on device (os error 28)"))
    }

    @Test fun pastaInacessivel() {
        assertEquals(
            "A pasta do arquivo de sincronização não está acessível.",
            adapt("A pasta /data/user/0/io.github.demusis.ponte/no_backup não está acessível.")
        )
        assertEquals(
            "O arquivo de sincronização não está acessível agora. Espere o Drive ficar disponível e tente de novo.",
            adapt(
                "Nenhuma pasta está acessível agora ($work: Permission denied). " +
                    "Conecte o pendrive ou espere a nuvem ficar disponível."
            )
        )
    }

    @Test fun nenhumaPastaGravada() = assertEquals(
        "Não foi possível gravar o arquivo de sincronização. As alterações continuam guardadas neste aparelho.",
        adapt(
            "Nenhuma pasta pôde ser gravada: Não foi possível gravar $work.. " +
                "As alterações continuam guardadas neste computador."
        )
    )

    @Test fun primeiraSincronizacaoComAlteracoesPendentes() = assertEquals(
        "Este aparelho ainda não sincronizou com este arquivo e já tem 2 documentos no acervo. " +
            "Em Ajustes, recomece o acervo deste aparelho e sincronize de novo.",
        adapt(
            "[confirmar-substituicao] Este computador ainda não sincronizou com esta pasta e tem 2 documentos no acervo. " +
                "Ao sincronizar pela primeira vez, o acervo daqui será substituído pelo da pasta (revisão 3 • gravada " +
                "por Servidor A em 09/11/2026 10:00). Faça uma cópia de segurança antes, se quiser guardá-los."
        )
    )

    @Test fun pastaCompartilhadaInativa() = assertEquals(
        "A sincronização não está ativa neste aparelho. Em Ajustes, toque em Trocar arquivo e escolha-o de novo.",
        adapt("A pasta compartilhada não está ativa neste computador.")
    )

    @Test fun resumoDaSincronizacao() {
        assertEquals(
            "primeira cópia criada a partir deste aparelho • gravado no Drive",
            adapt("primeira cópia criada a partir deste computador • gravado em 1 pasta (revisão 1)")
        )
        assertEquals(
            "2 alterações deste aparelho enviadas • 1 alteração de outras cópias reunida",
            adapt("2 alterações deste computador enviadas • 1 alteração de outras pastas reunida (revisão 7)")
        )
        assertEquals("em dia", adapt("em dia (revisão 5)"))
    }

    @Test fun conflitosSaoResolvidosNoComputador() {
        assertEquals(
            "1 conflito de edição para resolver no computador",
            adapt("⚠ 1 conflito de edição para decidir (revisão 4)")
        )
        assertEquals(
            "3 conflitos de edição para resolver no computador • gravado no Drive",
            adapt("⚠ 3 conflitos de edição para decidir • gravado em 1 pasta (revisão 9)")
        )
    }

    @Test fun caminhoDesconhecidoSai() = assertEquals(
        "Não deu certo com o arquivo.",
        adapt("Não deu certo com o arquivo $work.")
    )

    @Test fun semCaminhoNemPalavrasDoComputadorFicaIgual() {
        val texts = listOf(
            "Senha de app recusada pelo servidor.",
            "Cancelado por você.",
            "Conflito: no computador principal estava “Ofício 12/2026”.",
            "Ofício 12/2026 e/ou 13/2026 - versão de 10/11/2026",
            "O computador principal não entendeu esta mensagem.",
            "Nenhuma pasta no Drive: confira a conexão."
        )
        for (t in texts) assertEquals(t, adapt(t))
    }

    @Test fun datasEBarrasNaoSaoCaminhos() = assertEquals(
        "Importado em 10/11/2026 (ofícios/anexos)", adapt("Importado em 10/11/2026 (ofícios/anexos)")
    )

    @Test fun nenhumCaminhoSobra() {
        val out = adapt("Não foi possível ler $work.: Permission denied (os error 13)")
        assertFalse(out.contains("/data"))
        assertFalse(out.contains("os error"))
    }

    // ---------------------------------------------------------------- plural

    @Test fun plural() {
        assertEquals("1 tarefa", PhoneWords.plural(1, "tarefa", "tarefas"))
        assertEquals("0 tarefas", PhoneWords.plural(0, "tarefa", "tarefas"))
        assertEquals("3 tarefas", PhoneWords.plural(3, "tarefa", "tarefas"))
        assertEquals("1 tarefa atrasada ou para hoje", PhoneWords.plural(1, "tarefa atrasada ou para hoje", "tarefas atrasadas ou para hoje"))
    }

    // ---------------------------------------------------------------- failures of the system

    @Test fun semConexao() {
        assertEquals(PhoneWords.NO_CONNECTION, PhoneWords.explain(UnknownHostException("imap.exemplo.org")))
        assertEquals(PhoneWords.NO_CONNECTION, PhoneWords.explain(ConnectException("Connection refused")))
    }

    @Test fun demorouDemais() =
        assertEquals(PhoneWords.TIMEOUT, PhoneWords.explain(SocketTimeoutException("timeout")))

    @Test fun causaDeUmaExcecaoEmbrulhada() =
        assertEquals(PhoneWords.NO_CONNECTION, PhoneWords.explain(RuntimeException("x", UnknownHostException("h"))))

    @Test fun semEspaco() {
        assertEquals(PhoneWords.NO_SPACE, PhoneWords.explain(IOException("write failed: ENOSPC (No space left on device)")))
    }

    @Test fun arquivoDoDriveNaoEncontradoOuSemPermissao() {
        assertEquals(PhoneWords.FILE_NOT_FOUND, PhoneWords.explain(FileNotFoundException("content://x/y"), driveFile = true))
        assertEquals(PhoneWords.FILE_DENIED, PhoneWords.explain(SecurityException("Permission Denial"), driveFile = true))
    }

    @Test fun foraDoDriveArquivoNaoEncontradoVaiParaOTextoGeral() {
        assertEquals("Texto da ação.", PhoneWords.explain(FileNotFoundException("/x/y"), "Texto da ação."))
        assertEquals("Texto da ação.", PhoneWords.explain(SecurityException("x"), "Texto da ação."))
    }

    @Test fun textoEscritoParaAPessoaEMantido() {
        assertEquals("A foto é grande demais para ser guardada. Tire outra foto.",
            PhoneWords.explain(PonteException("A foto é grande demais para ser guardada. Tire outra foto.")))
        // A core message is worded for the phone as well.
        assertEquals(
            "Atualize o Ponte de bolso.", PhoneWords.explain(CoreException("Atualize o Ponte."), "Texto da ação.")
        )
    }

    @Test fun qualquerOutraFalhaViraOTextoDaAcao() {
        val e = IllegalStateException("Check failed.")
        assertEquals("Não foi possível criar a tarefa.", PhoneWords.explain(e, "Não foi possível criar a tarefa."))
        assertEquals(PhoneWords.GENERIC, PhoneWords.explain(NullPointerException()))
        assertFalse(PhoneWords.explain(e).contains("Check failed"))
    }
}
