package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Test

class NoticeTextTest {
    @Test fun addsCountsAndHandlesEmptyBase() {
        assertEquals("3 cobranças para hoje e 2 entregas em risco.", NoticeText.build("", 3, 2))
        assertEquals("1 cobrança para hoje.", NoticeText.build("", 1, 0))
        assertEquals("1 entrega em risco.", NoticeText.build("", 0, 1))
        assertEquals("1 tarefa para hoje e 2 entregas em risco.", NoticeText.build("1 tarefa para hoje.", 0, 2))
        assertEquals("", NoticeText.build("", 0, 0))
    }
}
