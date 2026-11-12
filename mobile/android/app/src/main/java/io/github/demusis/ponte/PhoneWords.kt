package io.github.demusis.ponte

import java.io.FileNotFoundException
import java.io.IOException
import java.io.InterruptedIOException
import java.net.ConnectException
import java.net.NoRouteToHostException
import java.net.SocketException
import java.net.SocketTimeoutException
import java.net.UnknownHostException

/** A failure whose message is already written for the person (no further rewriting needed). */
internal open class PonteException(message: String) : Exception(message)

/**
 * Words of the screen, free of Android types so plain JUnit can test them: the texts that come
 * from the shared core are written for the computer ("deste computador", a file path in the
 * message...) and are rewritten here for the phone; failures of the system become sentences.
 */
internal object PhoneWords {
    const val NO_CONNECTION = "Sem conexão com a internet. Confira a rede e tente de novo."
    const val TIMEOUT = "A conexão demorou demais. Tente de novo."
    const val NO_SPACE = "Não há espaço livre no aparelho. Libere espaço e tente de novo."
    const val NO_MEMORY = "A memória do aparelho não foi suficiente. Feche outros aplicativos e tente de novo."
    const val FILE_NOT_FOUND =
        "O arquivo de sincronização não foi encontrado no Drive. Em Ajustes, toque em Trocar arquivo e escolha-o de novo."
    const val FILE_DENIED =
        "O Ponte de bolso perdeu o acesso ao arquivo de sincronização. Em Ajustes, toque em Trocar arquivo e escolha-o de novo."
    const val INTERNAL =
        "Falha interna do Ponte de bolso. Feche e abra o Ponte de bolso; se continuar, sincronize pelo computador e avise quem cuida do Ponte."
    const val GENERIC = "Não foi possível concluir. Tente de novo; se continuar, feche e abra o Ponte de bolso."

    // Recursos (rooms, books, equipment and their uses): the words of the computer's Recursos area.
    // A booking or loan is a "uso", never a "registro" or an "atendimento".
    const val RES_AREA = "Recursos"
    const val RES_USES = "Usos"
    const val RES_USE_TITLE = "Uso"
    const val RES_RESOURCE_TITLE = "Recurso"
    const val RES_WHO = "Quem"
    const val RES_SECTOR = "Setor"
    const val RES_PURPOSE = "Finalidade"
    const val RES_START = "Início"
    const val RES_END = "Fim previsto"
    const val RES_RETURNED = "Devolvido em"
    const val RES_STATUS = "Situação"
    const val RES_NOTES = "Observações"
    const val RES_QUANTITY = "Quantidade"
    const val RES_LOCATION = "Onde fica"
    const val RES_IDENTIFICATION = "Identificação"
    const val RES_KIND = "Tipo"
    const val RES_OVERDUE = "Atrasado"
    const val RES_INACTIVE = "Fora de uso"
    const val RES_GROUP_LATE = "Atrasados"
    const val RES_GROUP_IN_USE = "Em uso agora"
    const val RES_GROUP_TODAY = "Hoje"
    const val RES_GROUP_NEXT = "Próximos dias"
    const val RES_GROUP_LATER = "Mais adiante"
    const val RES_GROUP_CLOSED = "Encerrados"
    const val RES_TODAY_LINE = "Reservas de hoje"
    const val RES_OVERDUE_LINE = "Devoluções atrasadas"
    const val RES_SEARCH_USES = "Buscar por recurso, quem ou finalidade"
    const val RES_SEARCH_RESOURCES = "Buscar por nome, tipo ou local"

    /** Every text above that names the thing, to check the vocabulary ("uso", never "registro"/"atendimento"). */
    val RES_TEXTS: List<String> = listOf(
        RES_AREA, RES_USES, RES_USE_TITLE, RES_RESOURCE_TITLE, RES_WHO, RES_SECTOR, RES_PURPOSE, RES_START, RES_END,
        RES_RETURNED, RES_STATUS, RES_NOTES, RES_QUANTITY, RES_LOCATION, RES_IDENTIFICATION, RES_KIND, RES_OVERDUE,
        RES_INACTIVE, RES_GROUP_LATE, RES_GROUP_IN_USE, RES_GROUP_TODAY, RES_GROUP_NEXT, RES_GROUP_LATER,
        RES_GROUP_CLOSED, RES_TODAY_LINE, RES_OVERDUE_LINE, RES_SEARCH_USES, RES_SEARCH_RESOURCES
    )

    /** "1 tarefa" or "3 tarefas". */
    fun plural(n: Int, one: String, many: String) = if (n == 1) "1 $one" else "$n $many"

    private val any = setOf(RegexOption.DOT_MATCHES_ALL)

    // A file-system path: Android style (/data/user/0/...) or Windows style (C:\...). Never
    // starts in the middle of a word, and leaves the sentence's own final punctuation alone.
    private const val SEG = """[^\s/"“”()]+"""
    private const val PATH = """(?<![\w/:.\-])(?:/(?:$SEG/)+(?:$SEG)?|[A-Za-z]:\\[^\s"“”()]*)(?<![.,;:])"""

    private val OS_ERROR = Regex("""\s*:\s*[^:]*?\(os error \d+\)""")
    private val ANY_PATH = Regex(PATH)

    /**
     * Messages of the core (sharedfolder.rs, intranet.rs, mobile/rust/src/lib.rs), most specific
     * first. Each one is matched whole, so nothing else is touched.
     */
    private val sentences: List<Pair<Regex, String>> = listOf(
        // mobile/rust/src/lib.rs:44
        Regex("""Falha interna no núcleo do Ponte\.""") to INTERNAL,
        // mobile/rust/src/lib.rs:139
        Regex("""O arquivo de sincronização não foi encontrado\.""") to FILE_NOT_FOUND,
        // mobile/rust/src/lib.rs:124
        Regex("""Digite a frase-senha da sincronização\.""") to "Digite a senha de sincronização.",
        // src/sharedfolder.rs:203
        Regex("""Senha de sincronização incorreta, ou arquivo danificado ou ainda sendo gravado pela nuvem\. Confira a senha e espere a pasta terminar de sincronizar\.""") to
            "Senha de sincronização incorreta, ou o arquivo está danificado ou ainda sendo gravado pelo Drive. " +
            "Confira a senha e espere o Drive terminar de sincronizar.",
        // src/intranet.rs:2324
        Regex("""A cópia em .+? foi gravada por um Ponte mais novo; atualize o Ponte neste computador\.""", any) to
            "O arquivo de sincronização foi gravado por uma versão mais nova do Ponte. Atualize o Ponte de bolso.",
        // src/sharedfolder.rs:164
        Regex("""Atualize o Ponte\.""") to "Atualize o Ponte de bolso.",
        // src/intranet.rs:2319
        Regex("""A pasta .+? contém outro acervo, não o deste computador\. Retire-a da lista ou escolha outro arquivo\.""", any) to
            "O arquivo de sincronização é de outro acervo, não o deste aparelho. " +
            "Em Ajustes, toque em Trocar arquivo e escolha o arquivo certo.",
        // src/sharedfolder.rs:131 and 180
        Regex("""Não foi possível ler $PATH\.""") to "Não foi possível ler o arquivo de sincronização.",
        // src/sharedfolder.rs:137 and 143
        Regex("""$PATH não é um arquivo de sincronização do Ponte\.""") to
            "O arquivo escolhido não é um arquivo de sincronização do Ponte. " +
            "Em Ajustes, toque em Trocar arquivo e escolha o arquivo certo.",
        // src/sharedfolder.rs:265
        Regex("""A pasta $PATH não está acessível\.""") to "A pasta do arquivo de sincronização não está acessível.",
        // src/sharedfolder.rs:287
        Regex("""Não foi possível gravar $PATH\.""") to "Não foi possível gravar o arquivo de sincronização.",
        // src/intranet.rs:2297
        Regex("""Nenhuma pasta está acessível agora \(.*?\)\. Conecte o pendrive ou espere a nuvem ficar disponível\.""", any) to
            "O arquivo de sincronização não está acessível agora. Espere o Drive ficar disponível e tente de novo.",
        // src/intranet.rs:2794
        Regex("""Nenhuma pasta pôde ser gravada: .*?\. As alterações continuam guardadas neste computador\.""", any) to
            "Não foi possível gravar o arquivo de sincronização. As alterações continuam guardadas neste aparelho.",
        // src/intranet.rs:2382
        Regex("""A pasta ainda não recebeu a versão mais recente \(revisão \d+ na pasta, \d+ neste computador\)\. Espere a nuvem terminar de sincronizar e tente de novo\.""") to
            "O Drive ainda não entregou a versão mais recente do arquivo de sincronização. " +
            "Espere o Drive terminar de sincronizar e tente de novo.",
        // src/intranet.rs:2396
        Regex("""\[confirmar-substituicao\] Este computador ainda não sincronizou com esta pasta e tem (.+?) no acervo\..*""", any) to
            "Este aparelho ainda não sincronizou com este arquivo e já tem $1 no acervo. " +
            "Em Ajustes, recomece o acervo deste aparelho e sincronize de novo.",
        // src/intranet.rs:2271
        Regex("""A pasta compartilhada não está ativa neste computador\.""") to
            "A sincronização não está ativa neste aparelho. Em Ajustes, toque em Trocar arquivo e escolha-o de novo.",
        // src/intranet.rs:4334 (the person cannot decide on the phone)
        Regex("""⚠ (\d+ conflitos? de edição) para decidir""") to "$1 para resolver no computador",
        // src/intranet.rs:4340: the phone has a single file, which is on the Drive
        Regex("""gravado em 1 pasta""") to "gravado no Drive",
        // src/intranet.rs:4323
        Regex("""de outras pastas""") to "de outras cópias",
        // src/intranet.rs:4351 and 4353: the revision number means nothing to the person
        Regex("""\s*\(revisão \d+\)""") to ""
    )

    /** Fragments of "this computer" (src/intranet.rs:2513..4312), for what is left. */
    private val words: List<Pair<String, String>> = listOf(
        "neste computador" to "neste aparelho",
        "deste computador" to "deste aparelho",
        "Este computador" to "Este aparelho",
        "este computador" to "este aparelho"
    )

    /**
     * Rewrites a text that came from the core (a status, a summary or an error, never document
     * data) for the phone; text it does not know is returned as it was.
     */
    fun adapt(text: String): String {
        if (text.contains("No space left on device")) return NO_SPACE
        var t = OS_ERROR.replace(text, "")
        for ((re, to) in sentences) t = re.replace(t, to)
        for ((from, to) in words) t = t.replace(from, to)
        if (ANY_PATH.containsMatchIn(t)) {
            t = ANY_PATH.replace(t, "")
            t = t.replace(Regex("""[ \t]{2,}"""), " ").replace(Regex("""\s+([.,;:])"""), "$1").trim()
        }
        return t
    }

    /**
     * A sentence for a failure caught on the phone. A message written for the person (ours, or
     * the core's) is kept; the failures of the system are told by kind; anything else becomes
     * [fallback]. [driveFile] says the failure happened reading or writing the Drive file.
     */
    fun explain(e: Throwable, fallback: String = GENERIC, driveFile: Boolean = false): String {
        var t: Throwable? = e
        var depth = 0
        while (t != null && depth < 6) {
            when {
                t is PonteException -> return adapt(t.message ?: fallback)
                t is OutOfMemoryError -> return NO_MEMORY
                t is SocketTimeoutException -> return TIMEOUT
                t is InterruptedIOException -> return TIMEOUT
                t is UnknownHostException || t is ConnectException || t is NoRouteToHostException -> return NO_CONNECTION
                t is FileNotFoundException && driveFile -> return FILE_NOT_FOUND
                t is SecurityException && driveFile -> return FILE_DENIED
                t is IOException && (t.message ?: "").let { it.contains("ENOSPC") || it.contains("No space left") } -> return NO_SPACE
                t is SocketException -> return NO_CONNECTION
            }
            t = t.cause
            depth++
        }
        return fallback
    }
}
