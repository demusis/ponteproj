package io.github.demusis.ponte

import java.time.LocalDate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Test

class HolidaysTest {
    @After fun voltaAoPadrao() = Holidays.configure("")

    private fun d(y: Int, m: Int, day: Int) = LocalDate.of(y, m, day)

    @Test fun pascoa() {
        assertEquals(d(2024, 3, 31), Holidays.easter(2024))
        assertEquals(d(2025, 4, 20), Holidays.easter(2025))
        assertEquals(d(2026, 4, 5), Holidays.easter(2026))
        assertEquals(d(2027, 3, 28), Holidays.easter(2027))
    }

    @Test fun feriadosMoveis2026() {
        assertEquals("Carnaval", Holidays.holiday(d(2026, 2, 16)))
        assertEquals("Carnaval", Holidays.holiday(d(2026, 2, 17)))
        assertNull(Holidays.holiday(d(2026, 2, 18)))
        assertEquals("Sexta-feira Santa", Holidays.holiday(d(2026, 4, 3)))
        assertEquals("Corpus Christi", Holidays.holiday(d(2026, 6, 4)))
    }

    @Test fun feriadosFixosEConscienciaNegra() {
        assertEquals("Tiradentes", Holidays.holiday(d(2026, 4, 21)))
        assertEquals("Natal", Holidays.holiday(d(2026, 12, 25)))
        assertNull(Holidays.holiday(d(2023, 11, 20)))
        assertEquals("Dia da Consciência Negra", Holidays.holiday(d(2026, 11, 20)))
    }

    @Test fun diasUteis() {
        assertTrue(Holidays.isWorkingDay(d(2026, 4, 22)))
        assertFalse(Holidays.isWorkingDay(d(2026, 4, 21)))
        assertFalse(Holidays.isWorkingDay(d(2026, 4, 25)))
        assertFalse(Holidays.isWorkingDay(d(2026, 11, 20)))
    }

    @Test fun textoVaiEVoltaEListaRuimEIgnorada() {
        val config = HolidayConfig.parse(
            "uf SP\nanual 09-07 Dia da cidade\ndia 2026-03-19 Ponte  do santo\nanual 02-29\n"
        )
        assertEquals("SP", config.uf)
        assertEquals(3, config.custom.size)
        assertEquals("Ponte  do santo", config.custom[1].name)
        assertEquals("", config.custom[2].name)
        assertEquals(config, HolidayConfig.parse(config.toText()))
        val lixo = HolidayConfig.parse(
            "lixo\nuf XX\nanual 13-40 x\nanual abc\ndia ontem x\ndia\nuf\n\u0000\u0000\n   \nfoo bar baz"
        )
        assertEquals(HolidayConfig(), lixo)
        assertEquals(HolidayConfig(), HolidayConfig.parse(""))
    }

    @Test fun copiaDoAcervoFicaNoTextoCanonicoELimpaOQueNaoEntende() {
        // O que vem do acervo (de outro aparelho) vira a cópia local sem lixo e sem mudar o sentido.
        val doAcervo = "lixo\nuf SP\nanual 09-07   Dia da cidade \ndia 2026-03-19 Ponte\nuf XX\n"
        val canonico = Holidays.canonical(doAcervo)
        // O último "uf" vale (aqui, um estado que não existe: nenhum).
        assertEquals("anual 09-07 Dia da cidade\ndia 2026-03-19 Ponte\n", canonico)
        assertEquals(HolidayConfig.parse(doAcervo), HolidayConfig.parse(canonico))
        // Texto canônico é ponto fixo: guardar de novo não muda nada.
        assertEquals(canonico, Holidays.canonical(canonico))
        // Acervo com os feriados limpos de propósito é um texto vazio, não "sem configuração".
        assertEquals("", Holidays.canonical(""))
        assertEquals("", Holidays.canonical("lixo\n"))
    }

    @Test fun avisoDeAjustesDizQueValePeloAcervo() {
        assertTrue(HOLIDAYS_NOTE.contains("todos os aparelhos que sincronizam este acervo"))
        assertFalse(HOLIDAYS_NOTE.contains("só neste"))
    }

    @Test fun diaDeSaoPauloSoContaEmSaoPaulo() {
        // 9/7/2026 é quinta-feira.
        assertTrue(Holidays.isWorkingDay(d(2026, 7, 9)))
        Holidays.configure("uf RJ")
        assertTrue(Holidays.isWorkingDay(d(2026, 7, 9)))
        Holidays.configure("uf SP")
        assertFalse(Holidays.isWorkingDay(d(2026, 7, 9)))
        assertEquals("Revolução Constitucionalista", Holidays.holiday(d(2026, 7, 9)))
    }

    @Test fun penhaDoEspiritoSantoEhASegundaDepoisDaSemanaDaPascoa() {
        Holidays.configure("uf ES")
        assertEquals("Nossa Senhora da Penha", Holidays.holiday(d(2026, 4, 13)))
        assertNull(Holidays.holiday(d(2026, 4, 12)))
        Holidays.configure("")
        assertNull(Holidays.holiday(d(2026, 4, 13)))
    }

    @Test fun dataMagnaDePernambucoComecaEm2017() {
        Holidays.configure("uf PE")
        assertNull(Holidays.holiday(d(2016, 3, 6)))
        assertTrue(Holidays.holiday(d(2017, 3, 6)) != null)
        assertTrue(Holidays.holiday(d(2026, 6, 24)) != null)
    }

    @Test fun nacionalVemPrimeiroETodosOsEstadosSaoConhecidos() {
        assertEquals(27, Holidays.STATES.size)
        Holidays.configure("uf MG\nanual 04-21 Outro")
        assertEquals("Tiradentes", Holidays.holiday(d(2026, 4, 21)))
        for ((code, full) in Holidays.STATES) assertEquals(full, Holidays.stateName(code))
        assertTrue(Holidays.stateHolidayLines("GO").isEmpty())
        assertEquals(4, Holidays.stateHolidayLines("AC").size)
        assertEquals("09/07", Holidays.stateHolidayLines("SP")[0].first)
        assertEquals("8 dias depois da Páscoa", Holidays.stateHolidayLines("ES")[0].first)
    }

    @Test fun diasCadastradosAnuaisEUnicos() {
        Holidays.configure("anual 03-19 São José\ndia 2026-05-04 Recesso\nanual 06-10\n")
        assertEquals("São José", Holidays.holiday(d(2027, 3, 19)))
        assertEquals("Recesso", Holidays.holiday(d(2026, 5, 4)))
        assertNull(Holidays.holiday(d(2027, 5, 4)))
        assertEquals("Dia sem expediente", Holidays.holiday(d(2026, 6, 10)))
        assertFalse(Holidays.isWorkingDay(d(2026, 5, 4)))
        assertTrue(Holidays.isWorkingDay(d(2026, 5, 5)))
    }

    @Test fun fimAutomaticoPulaFeriadoEstadual() {
        // Quarta 8/7/2026 mais 2 dias: sem estado termina na quinta 9; em São Paulo, a quinta é feriado.
        assertEquals("2026-07-09T18:00", TaskRules.autoEnd("2026-07-08T09:00", 2, ""))
        Holidays.configure("uf SP")
        assertEquals("2026-07-10T18:00", TaskRules.autoEnd("2026-07-08T09:00", 2, ""))
    }
}
