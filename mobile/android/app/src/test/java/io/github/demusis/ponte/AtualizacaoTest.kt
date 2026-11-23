package io.github.demusis.ponte

import androidx.compose.foundation.layout.Column
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

class AtualizacaoTest {
    @get:Rule val paparazzi = newPaparazzi()
    @Test fun comparaVersoes() {
        val v = { s: String -> parseVersion(s)!! }
        assertTrue(v("1.10.0") > v("1.9.9"))
        assertTrue(v("1.0.0") > v("1.0.0-rc.1"))
        assertTrue(v("1.0.0-rc.10") > v("1.0.0-rc.2"))
        assertTrue(v("1.0.0-alpha") < v("1.0.0-alpha.1"))
        assertEquals(v("v1.2.3+build"), v("1.2.3"))
        assertTrue(v("1.2.2") < v("1.2.3"))
        listOf("1.2", "1.2.3-01", "1.2.3-", "01.2.3", "1.2.3.4").forEach { assertNull(parseVersion(it)) }
    }
    @Test fun escolheApk() {
        assertNull(selectApkVersion(listOf("PonteDeBolso-1.2.3.zip", "outro-1.2.3.apk")))
        assertEquals("v1.2.3", selectApkVersion(listOf("PonteDeBolso-1.2.3-rc.1.apk", "PonteDeBolso-v1.2.3.apk", "PonteDeBolso-1.2.2.apk")))
    }
    @Test fun paginaConfiavel() {
        val good = "https://github.com/demusis/ponteproj/releases/tag/v1.2.3"
        assertEquals(good, UpdateCheck.safeUrl(good))
        assertEquals("https://github.com/demusis/ponteproj/releases/latest", UpdateCheck.safeUrl("https://github.com/demusis/ponteproj/releases/../../outro"))
    }
    @Test fun sobreAtual() = paparazzi.shot("sobre_atual") { Column { AboutContent("1.2.3", UpdateResult.Current("1.2.3")) } }
    @Test fun sobreNova() = paparazzi.shot("sobre_nova") { Column { AboutContent("1.2.3", UpdateResult.Newer("1.3.0", "https://github.com/demusis/ponteproj/releases/latest")) } }
    @Test fun sobreErro() = paparazzi.shot("sobre_erro") { Column { AboutContent("1.2.3", UpdateResult.Error) } }
}
