package io.github.demusis.ponte

import io.github.demusis.ponte.TaskReminders.Kind
import io.github.demusis.ponte.TaskReminders.Ledger
import io.github.demusis.ponte.TaskReminders.Task
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime

/** The individual notices of the tasks are plain Kotlin (no phone, no library): fixed dates, invented tasks. */
class AvisosTarefasTest {
    private val sp = ZoneId.of("America/Sao_Paulo")

    private fun at(text: String, zone: ZoneId = sp): ZonedDateTime = ZonedDateTime.of(LocalDateTime.parse(text), zone)

    private fun task(
        uid: String = "u1", title: String = "Revisar minuta", start: String = "2026-11-18T09:00", due: String = "",
        offsets: List<Int> = listOf(30), done: Boolean = false
    ) = Task(uid, title, start, due, offsets, done)

    /** One round as the phone does it: plan, commit, save (encode) — and the next plan after a restart (decode). */
    private fun round(tasks: List<Task>, ledger: Ledger, now: ZonedDateTime): TaskReminders.Commit {
        val plan = TaskReminders.plan(tasks, ledger, now)
        return TaskReminders.commit(tasks, ledger, plan, now)
    }

    // ---------------------------------------------------------------- the moments

    @Test fun startIsTheAnchorAndOffsetsCountBackFromIt() {
        val occ = TaskReminders.occurrences(listOf(task(offsets = listOf(0, 15, 60))), sp)
        assertEquals(listOf(60, 15, 0), occ.map { it.offset })
        assertEquals(listOf(at("2026-11-18T08:00"), at("2026-11-18T08:45"), at("2026-11-18T09:00")), occ.map { it.trigger })
        assertTrue(occ.all { it.kind == Kind.START })
    }

    @Test fun withoutStartTheDueDateAtNineIsTheAnchor() {
        val occ = TaskReminders.occurrences(listOf(task(start = "", due = "2026-11-20", offsets = listOf(1440, 0))), sp)
        assertEquals(listOf(at("2026-11-19T09:00"), at("2026-11-20T09:00")), occ.map { it.trigger })
        assertTrue(occ.all { it.kind == Kind.DUE })
    }

    @Test fun anInvalidStartFallsBackToTheDueDateAndNoAnchorMeansNoNotice() {
        assertEquals(Kind.DUE, TaskReminders.anchorOf(task(start = "2026-11-18", due = "2026-11-20"))!!.first)
        assertEquals(Kind.DUE, TaskReminders.anchorOf(task(start = "2026-13-40T09:00", due = "2026-11-20"))!!.first)
        assertNull(TaskReminders.anchorOf(task(start = "", due = "")))
        assertNull(TaskReminders.anchorOf(task(start = "", due = "2026-02-30")))
        assertTrue(TaskReminders.occurrences(listOf(task(start = "", due = "")), sp).isEmpty())
    }

    @Test fun offsetsAreCleaned() {
        assertEquals(listOf(60, 10, 0), TaskReminders.cleanOffsets(listOf(10, -5, 60, 10, 0, TaskReminders.MAX_OFFSET_MINUTES + 1)))
        assertEquals(10, TaskReminders.cleanOffsets((1..20).toList()).size)
        assertTrue(TaskReminders.occurrences(listOf(task(offsets = emptyList())), sp).isEmpty())
    }

    @Test fun theLimitsAreTheCoresAndNothingTheCoreAcceptsIsDropped() {
        // mobile/rust and src/tasks.rs: at most ten offsets, each up to 525600 minutes.
        assertEquals(10, TaskReminders.MAX_OFFSETS)
        assertEquals(525_600, TaskReminders.MAX_OFFSET_MINUTES)
        val ten = listOf(0, 5, 10, 15, 30, 60, 120, 1440, 10_080, 525_600)
        assertEquals(ten.sortedDescending(), TaskReminders.cleanOffsets(ten))
        assertEquals(10, TaskReminders.occurrences(listOf(task(offsets = ten)), sp).size)
        assertTrue(TaskReminders.cleanOffsets(listOf(525_601)).isEmpty())
    }

    @Test fun aReminderMonthsAheadIsArmedByALaterRefreshAndToldWhenReached() {
        // Start in 200 days, reminders 30 days and 0 min before it: there is no horizon to fall off.
        val tasks = listOf(task(start = "2027-06-06T09:00", offsets = listOf(0, 43_200)))
        val now = at("2026-11-18T08:00")
        val first = round(tasks, Ledger(), now)
        assertTrue(first.shown.isEmpty())
        // Arms the nearest moment, however far: 30 days before the start.
        val nearest = TaskReminders.plan(tasks, first.ledger, now).next
        assertEquals(at("2027-05-07T09:00"), nearest)
        // That refresh tells it and arms the next one (the start itself).
        val second = round(tasks, first.ledger, nearest!!.plusMinutes(1))
        assertEquals(listOf(43_200), second.shown.map { it.occurrence.offset })
        assertEquals(at("2027-06-06T09:00"), TaskReminders.plan(tasks, second.ledger, nearest.plusMinutes(2)).next)
        val third = round(tasks, second.ledger, at("2027-06-06T09:00"))
        assertEquals(listOf(0), third.shown.map { it.occurrence.offset })
    }

    @Test fun doneTasksAndTasksWithoutAUsableUidHaveNoNotices() {
        assertTrue(TaskReminders.occurrences(listOf(task(done = true)), sp).isEmpty())
        assertTrue(TaskReminders.occurrences(listOf(task(uid = "")), sp).isEmpty())
        assertTrue(TaskReminders.occurrences(listOf(task(uid = "a|b")), sp).isEmpty())
        assertTrue(TaskReminders.occurrences(listOf(task(uid = "a\tb")), sp).isEmpty())
    }

    @Test fun theKeyIsUidAnchorOffsetAndSurvivesAParse() {
        val o = TaskReminders.occurrences(listOf(task(uid = "abc-123")), sp).single()
        assertEquals("abc-123|2026-11-18T09:00|30", o.key)
        assertEquals("abc-123", TaskReminders.uidOfKey(o.key))
        assertNull(TaskReminders.uidOfKey("sem-barras"))
        assertNull(TaskReminders.uidOfKey("|2026-11-18T09:00|30"))
    }

    // ---------------------------------------------------------------- the plan

    @Test fun nothingIsDueBeforeTheMomentAndTheAlarmIsForTheNearest() {
        val tasks = listOf(task(uid = "a", offsets = listOf(30)), task(uid = "b", start = "2026-11-18T08:00", offsets = listOf(0)))
        val plan = TaskReminders.plan(tasks, Ledger(), at("2026-11-18T06:00"))
        assertTrue(plan.show.isEmpty())
        assertTrue(plan.silent.isEmpty())
        assertEquals(at("2026-11-18T08:00"), plan.next)
    }

    @Test fun aReachedMomentIsToldOnce() {
        val tasks = listOf(task())
        val now = at("2026-11-18T08:31")
        val first = round(tasks, Ledger(), now)
        assertEquals(1, first.shown.size)
        assertEquals(TaskReminders.FIRST_ID, first.shown[0].id)
        // The very next look (a sync, the app opening) tells nothing more.
        val second = round(tasks, first.ledger, now.plusMinutes(1))
        assertTrue(second.shown.isEmpty())
        assertNull(TaskReminders.plan(tasks, second.ledger, now.plusMinutes(2)).next)
    }

    @Test fun aRestartDoesNotRepeatWhatWasTold() {
        val tasks = listOf(task(offsets = listOf(60, 30, 0)))
        val first = round(tasks, Ledger(), at("2026-11-18T08:35")) // 60 and 30 are due
        assertEquals(2, first.shown.size)
        val saved = first.ledger.encode()
        // New process: the ledger comes back from the text, the same instant is planned again.
        val restarted = Ledger.decode(saved)
        assertEquals(first.ledger, restarted)
        assertTrue(TaskReminders.plan(tasks, restarted, at("2026-11-18T08:36")).show.isEmpty())
        // Only the moment still ahead is left.
        assertEquals(at("2026-11-18T09:00"), TaskReminders.plan(tasks, restarted, at("2026-11-18T08:36")).next)
        val third = round(tasks, restarted, at("2026-11-18T09:01"))
        assertEquals(listOf(0), third.shown.map { it.occurrence.offset })
    }

    @Test fun aMissedNoticeIsToldWithinTheCatchUpAndSilentAfterIt() {
        val tasks = listOf(task(offsets = listOf(0)))
        val inside = TaskReminders.plan(tasks, Ledger(), at("2026-11-18T14:59"))
        assertEquals(1, inside.show.size)
        val edge = TaskReminders.plan(tasks, Ledger(), at("2026-11-18T15:01")) // 6 h and 1 min late
        assertTrue(edge.show.isEmpty())
        assertEquals(1, edge.silent.size)
        // The silent one is recorded, so it never comes back as a notice.
        val c = TaskReminders.commit(tasks, Ledger(), edge, at("2026-11-18T15:01"))
        assertTrue(c.shown.isEmpty())
        assertTrue(TaskReminders.plan(tasks, c.ledger, at("2026-11-18T15:02")).show.isEmpty())
        assertEquals(0, c.ledger.entries.values.single().id)
    }

    @Test fun aTaskThatArrivesLongPastAndAncientMomentsMakeNoNoise() {
        val tasks = listOf(task(start = "2026-09-01T09:00", offsets = listOf(0)))
        val plan = TaskReminders.plan(tasks, Ledger(), at("2026-11-18T08:00")) // 78 days later
        assertTrue(plan.show.isEmpty())
        assertTrue(plan.silent.isEmpty()) // older than the memory: not even recorded
        assertNull(plan.next)
    }

    @Test fun concludedTasksAreSilentAndTheirNotificationIsTakenOff() {
        val open = listOf(task())
        val told = round(open, Ledger(), at("2026-11-18T08:31"))
        val id = told.shown.single().id
        val concluded = listOf(task(done = true))
        val after = round(concluded, told.ledger, at("2026-11-18T08:40"))
        assertTrue(after.shown.isEmpty())
        assertEquals(listOf(id), after.cancel)
        assertEquals(0, after.ledger.entries.values.single().id)
        // A task concluded before its moment is never told.
        val early = TaskReminders.plan(concluded, Ledger(), at("2026-11-18T09:30"))
        assertTrue(early.show.isEmpty() && early.silent.isEmpty() && early.next == null)
    }

    @Test fun aMovedTaskDropsTheOldMomentAndAnnouncesTheNew() {
        val told = round(listOf(task()), Ledger(), at("2026-11-18T08:31"))
        val moved = listOf(task(start = "2026-11-19T10:00"))
        val after = round(moved, told.ledger, at("2026-11-18T08:45"))
        assertEquals(told.shown.map { it.id }, after.cancel) // the old notification goes away
        assertTrue(after.shown.isEmpty())
        assertEquals(at("2026-11-19T09:30"), TaskReminders.plan(moved, after.ledger, at("2026-11-18T08:45")).next)
        val next = round(moved, after.ledger, at("2026-11-19T09:31"))
        assertEquals("u1|2026-11-19T10:00|30", next.shown.single().occurrence.key)
    }

    @Test fun twoTasksAtTheSameMomentAreTwoNotices() {
        val tasks = listOf(task(uid = "a"), task(uid = "b"))
        val c = round(tasks, Ledger(), at("2026-11-18T08:30"))
        assertEquals(2, c.shown.size)
        assertEquals(2, c.shown.map { it.id }.toSet().size)
    }

    // ---------------------------------------------------------------- the ids

    @Test fun idsAreTheSmallestFreeNumberNeverAHash() {
        var l = Ledger()
        assertEquals(1000, l.freeId())
        l = l.put("a|2026-11-18T09:00|0", TaskReminders.Entry(1000, 1L)).put("b|2026-11-18T09:00|0", TaskReminders.Entry(1001, 1L))
        assertEquals(1002, l.freeId())
        l = l.put("c|2026-11-18T09:00|0", TaskReminders.Entry(0, 1L)) // past, silent: holds no id
        assertEquals(1002, l.freeId())
        l = l.put("a|2026-11-18T09:00|0", TaskReminders.Entry(0, 1L)) // taken off: 1000 is free again
        assertEquals(1000, l.freeId())
    }

    @Test fun manyUidsNeverCollideOnTheSameMoment() {
        val tasks = (1..300).map { task(uid = "uid-$it", offsets = listOf(0), start = "2026-11-18T09:00") }
        val now = at("2026-11-18T09:00")
        var ledger = Ledger()
        val ids = HashSet<Int>()
        // Each round shows at most five; a restart between rounds (encode/decode) changes nothing.
        repeat(60) {
            val c = round(tasks, Ledger.decode(ledger.encode()), now)
            c.shown.forEach { assertTrue("id repeated: ${it.id}", ids.add(it.id)) }
            ledger = c.ledger
        }
        assertEquals(300, ledger.entries.size)
        val withId = ledger.entries.values.filter { it.id > 0 }
        assertEquals(withId.size, withId.map { it.id }.toSet().size)
    }

    @Test fun moreThanFiveAtOnceAreFoldedIntoACount() {
        val tasks = (1..8).map { task(uid = "t$it", offsets = listOf(0)) }
        val c = round(tasks, Ledger(), at("2026-11-18T09:00"))
        assertEquals(TaskReminders.MAX_AT_ONCE, c.shown.size)
        assertEquals(3, c.folded)
        assertEquals("Mais 3 avisos de tarefas.", TaskReminders.foldedLine(c.folded))
        assertEquals("Mais 1 aviso de tarefa.", TaskReminders.foldedLine(1))
        // None of the eight is told again.
        assertTrue(TaskReminders.plan(tasks, c.ledger, at("2026-11-18T09:05")).show.isEmpty())
    }

    // ---------------------------------------------------------------- the ledger

    @Test fun theLedgerHoldsKeysAndNumbersNeverATitle() {
        val secret = "Parecer sigiloso do caso Fulano"
        val c = round(listOf(task(title = secret)), Ledger(), at("2026-11-18T08:31"))
        val text = c.ledger.encode()
        assertFalse(text.contains(secret))
        assertFalse(text.contains("Fulano"))
        assertTrue(text.contains("u1|2026-11-18T09:00|30"))
    }

    @Test fun aDamagedLedgerIsDroppedNotThrown() {
        assertTrue(Ledger.decode(null).entries.isEmpty())
        assertTrue(Ledger.decode("").entries.isEmpty())
        assertTrue(Ledger.decode("outra coisa\n1\t2\t3\tu|x|0").entries.isEmpty())
        val l = Ledger.decode("ledger1\nlixo\n1000\t5\t0\tu1|2026-11-18T09:00|30\nx\ty\tz\tw\n-4\t5\t0\tu2|2026-11-18T09:00|0\n1001\t5\t0\tsem-chave")
        assertEquals(listOf("u1|2026-11-18T09:00|30"), l.entries.keys.toList())
    }

    @Test fun oldEntriesAreForgotten() {
        val now = at("2026-11-18T09:00")
        val old = now.minusDays(31).toInstant().toEpochMilli()
        val recent = now.minusDays(2).toInstant().toEpochMilli()
        val l = Ledger().put("a|2026-10-01T09:00|0", TaskReminders.Entry(1000, old)).put("b|2026-11-16T09:00|0", TaskReminders.Entry(1001, recent))
        val c = TaskReminders.commit(emptyList(), l, TaskReminders.Plan(emptyList(), emptyList(), null), now)
        assertEquals(listOf("b|2026-11-16T09:00|0"), c.ledger.entries.keys.toList())
    }

    // ---------------------------------------------------------------- snooze

    @Test fun snoozeBringsTheNoticeBackOnceTenMinutesLater() {
        val tasks = listOf(task())
        val now = at("2026-11-18T08:31")
        val told = round(tasks, Ledger(), now)
        val key = told.shown.single().occurrence.key
        val snoozed = TaskReminders.snooze(told.ledger, key, now)!!
        val plan = TaskReminders.plan(tasks, snoozed, now.plusMinutes(1))
        assertTrue(plan.show.isEmpty())
        assertEquals(now.plusMinutes(10), plan.next)
        val again = round(tasks, snoozed, now.plusMinutes(10))
        assertEquals(1, again.shown.size)
        assertTrue(again.shown[0].snoozed)
        assertEquals(told.shown[0].id, again.shown[0].id) // replaces the same notification
        assertTrue(round(tasks, again.ledger, now.plusMinutes(11)).shown.isEmpty()) // and only once
    }

    @Test fun snoozeOfAnUnknownKeyChangesNothingAndASnoozedConcludedTaskStaysSilent() {
        assertNull(TaskReminders.snooze(Ledger(), "x|2026-11-18T09:00|0", at("2026-11-18T09:00")))
        val now = at("2026-11-18T08:31")
        val told = round(listOf(task()), Ledger(), now)
        val snoozed = TaskReminders.snooze(told.ledger, told.shown.single().occurrence.key, now)!!
        val after = round(listOf(task(done = true)), snoozed, now.plusMinutes(12))
        assertTrue(after.shown.isEmpty())
        assertTrue(after.ledger.entries.values.single().snoozeUntilMillis == 0L)
    }

    // ---------------------------------------------------------------- the clock and the zone

    @Test fun aZoneChangeNeverRepeatsAndMovesTheNextInstant() {
        val tasks = listOf(task(offsets = listOf(0, 120)))
        val told = round(tasks, Ledger(), at("2026-11-18T07:05")) // the 120-min one
        assertEquals(1, told.shown.size)
        // The person flies to Lisbon (3 h ahead): same local wall clock, another instant. Nothing is repeated;
        // the notice still ahead is at 09:00 Lisbon time.
        val lisbon = ZoneId.of("Europe/Lisbon")
        val now = ZonedDateTime.of(LocalDateTime.parse("2026-11-18T07:30"), lisbon)
        val plan = TaskReminders.plan(tasks, told.ledger, now)
        assertTrue(plan.show.isEmpty())
        assertEquals(ZonedDateTime.of(LocalDateTime.parse("2026-11-18T09:00"), lisbon), plan.next)
    }

    @Test fun aMomentInTheClockGapMovesForward() {
        val ny = ZoneId.of("America/New_York")
        val occ = TaskReminders.occurrences(listOf(task(start = "2026-03-08T02:30", offsets = listOf(0))), ny).single()
        assertEquals(LocalTime.of(3, 30), occ.trigger.toLocalTime())
        // Calendar arithmetic: a day with 23 hours still lands on the chosen local time.
        val day = TaskReminders.occurrences(listOf(task(start = "2026-03-09T09:00", offsets = listOf(1440))), ny).single()
        assertEquals(LocalDateTime.parse("2026-03-08T09:00"), day.trigger.toLocalDateTime())
    }

    @Test fun retryIsSoonAndNeverForever() {
        val now = at("2026-11-18T09:00")
        val last = now.minusMinutes(1).toInstant().toEpochMilli()
        assertEquals(now.plusMinutes(15), TaskReminders.retryAt(last, now))
        assertNull(TaskReminders.retryAt(0L, now))
        assertNull(TaskReminders.retryAt(now.minusHours(7).toInstant().toEpochMilli(), now))
    }

    // ---------------------------------------------------------------- the words

    @Test fun theLineNeverCarriesTheTitle() {
        val today = LocalDate.of(2026, 11, 18)
        fun line(t: Task, offset: Int = 0) = TaskReminders.line(TaskReminders.occurrences(listOf(t.copy(offsets = listOf(offset))), sp).single(), today)
        assertEquals("Começa hoje às 09:00.", line(task(title = "Segredo")))
        assertEquals("Começa amanhã às 14:30.", line(task(start = "2026-11-19T14:30")))
        assertEquals("Começa em 25/11 às 09:00.", line(task(start = "2026-11-25T09:00")))
        assertEquals("Prazo hoje.", line(task(start = "", due = "2026-11-18")))
        assertEquals("Prazo amanhã.", line(task(start = "", due = "2026-11-19")))
        assertEquals("Prazo: 30/11.", line(task(start = "", due = "2026-11-30")))
        assertFalse(line(task(title = "Segredo")).contains("Segredo"))
    }

    @Test fun theTitleIsHiddenWhenTheAppLockIsInForce() {
        assertEquals(TaskReminders.GENERIC_TITLE, TaskReminders.title("  Revisar minuta ", lockInForce = false))
        assertEquals(TaskReminders.GENERIC_TITLE, TaskReminders.title("Revisar minuta", lockInForce = true))
        assertEquals(TaskReminders.GENERIC_TITLE, TaskReminders.title("   ", lockInForce = false))
        assertEquals(TaskReminders.GENERIC_TITLE, TaskReminders.title(null, lockInForce = false))
        assertEquals(TaskReminders.GENERIC_TITLE, TaskReminders.title("x".repeat(200), lockInForce = false))
        assertFalse(TaskReminders.PUBLIC_TEXT.contains("minuta"))
    }

    // ---------------------------------------------------------------- the week

    private val monday = LocalDate.of(2026, 11, 16)

    @Test fun weekStartsOnMonday() {
        assertEquals(monday, WeekPlan.mondayOf(LocalDate.of(2026, 11, 18)))
        assertEquals(monday, WeekPlan.mondayOf(monday))
        assertEquals(monday, WeekPlan.mondayOf(LocalDate.of(2026, 11, 22)))
        assertEquals(7, WeekPlan.days(monday, emptyList()).size)
    }

    @Test fun theShortWeekTitleFitsAHeader() {
        assertEquals("16 – 22 nov 2026", WeekPlan.shortWeekTitle(monday))
        assertEquals("30 nov – 6 dez 2026", WeekPlan.shortWeekTitle(LocalDate.of(2026, 11, 30)))
        assertEquals("28 dez 2026 – 3 jan 2027", WeekPlan.shortWeekTitle(LocalDate.of(2026, 12, 28)))
    }

    @Test fun theDayStripHelpers() {
        assertEquals("Quarta, 18 de novembro", WeekPlan.longDayTitle(LocalDate.of(2026, 11, 18)))
        assertEquals("Domingo, 3 de janeiro", WeekPlan.longDayTitle(LocalDate.of(2027, 1, 3)))
        assertEquals(LocalDate.of(2026, 11, 18), WeekPlan.defaultSelected(monday, LocalDate.of(2026, 11, 18)))
        assertEquals(monday, WeekPlan.defaultSelected(monday, LocalDate.of(2026, 11, 23)))
        assertEquals(monday, WeekPlan.defaultSelected(monday, LocalDate.of(2026, 11, 9)))
        assertEquals(LocalDate.of(2026, 11, 22), WeekPlan.defaultSelected(monday, LocalDate.of(2026, 11, 22)))
        val entries = WeekPlan.task("u1", "A", "2026-11-17T14:00", "", 30, "2026-11-18")
        val days = WeekPlan.days(monday, entries)
        assertEquals(false to false, WeekPlan.dayMarks(days[0]))
        assertEquals(true to false, WeekPlan.dayMarks(days[1]))
        assertEquals(false to true, WeekPlan.dayMarks(days[2]))
    }

    @Test fun theWeekTitleCrossesMonthsAndYears() {
        assertEquals("16/11 a 22/11/2026", WeekPlan.weekTitle(monday))
        assertEquals("28/12 a 03/01/2027", WeekPlan.weekTitle(LocalDate.of(2026, 12, 28)))
    }

    @Test fun aTaskIsABlockAndItsDueDateIsAnotherThingOnAnotherDay() {
        val entries = WeekPlan.task("u1", "Audiência", "2026-11-17T14:00", "2026-11-17T15:30", 0, "2026-11-20")
        val days = WeekPlan.days(monday, entries)
        val tue = days[1]
        assertEquals(1, tue.blocks.size)
        assertEquals("14:00–15:30", WeekPlan.label(tue.blocks[0]))
        assertTrue(tue.deadlines.isEmpty())
        assertEquals(1, days[4].deadlines.size)
        assertTrue(days[4].blocks.isEmpty())
        assertTrue(days[0].isEmpty && days[2].isEmpty)
    }

    @Test fun theEndComesFromTheEstimateOrIsAPoint() {
        val withEstimate = WeekPlan.task("a", "A", "2026-11-17T09:00", "", 45, "").single()
        assertEquals(LocalDateTime.parse("2026-11-17T09:45"), withEstimate.end)
        val endBeforeStart = WeekPlan.task("b", "B", "2026-11-17T09:00", "2026-11-17T08:00", 0, "").single()
        assertEquals(endBeforeStart.start, endBeforeStart.end)
        assertEquals("09:00", WeekPlan.label(WeekPlan.days(monday, listOf(endBeforeStart))[1].blocks.single()))
        assertTrue(WeekPlan.task("c", "C", "", "", 30, "").isEmpty())
        assertTrue(WeekPlan.task("d", "D", "2026-02-30T09:00", "", 30, "2026-13-01").isEmpty())
    }

    @Test fun aBlockIsCutAtMidnight() {
        val entry = WeekEntry("u1", "Plantão", WeekKind.USE, LocalDateTime.parse("2026-11-17T22:00"), LocalDateTime.parse("2026-11-19T06:00"))
        val days = WeekPlan.days(monday, listOf(entry))
        val tue = days[1].blocks.single()
        assertEquals(LocalTime.of(22, 0), tue.from)
        assertEquals(LocalTime.MAX, tue.to)
        assertTrue(tue.continuesAfter && !tue.continuesBefore)
        assertEquals("a partir das 22:00", WeekPlan.label(tue))
        val wed = days[2].blocks.single()
        assertTrue(wed.continuesBefore && wed.continuesAfter)
        assertEquals("o dia todo", WeekPlan.label(wed))
        val thu = days[3].blocks.single()
        assertEquals(LocalTime.of(6, 0), thu.to)
        assertTrue(thu.continuesBefore && !thu.continuesAfter)
        assertEquals("até 06:00", WeekPlan.label(thu))
        assertTrue(days[0].isEmpty && days[4].isEmpty)
    }

    @Test fun anEndExactlyAtMidnightDoesNotLeakIntoTheNextDay() {
        val entry = WeekEntry("u1", "Noite", WeekKind.USE, LocalDateTime.parse("2026-11-17T20:00"), LocalDateTime.parse("2026-11-18T00:00"))
        val days = WeekPlan.days(monday, listOf(entry))
        assertEquals(1, days[1].blocks.size)
        assertEquals(LocalTime.MAX, days[1].blocks[0].to)
        assertFalse(days[1].blocks[0].continuesAfter)
        assertTrue(days[2].isEmpty)
        // A point exactly at midnight belongs to the day that starts there.
        val point = WeekEntry("p", "Ponto", WeekKind.TASK, LocalDateTime.parse("2026-11-18T00:00"), LocalDateTime.parse("2026-11-18T00:00"))
        val d2 = WeekPlan.days(monday, listOf(point))
        assertTrue(d2[1].isEmpty)
        assertEquals(1, d2[2].blocks.size)
    }

    @Test fun blocksOfADayAreOrderedByStart() {
        val a = WeekEntry("a", "Tarde", WeekKind.TASK, LocalDateTime.parse("2026-11-17T15:00"), LocalDateTime.parse("2026-11-17T16:00"))
        val b = WeekEntry("b", "Manhã", WeekKind.TASK, LocalDateTime.parse("2026-11-17T08:00"), LocalDateTime.parse("2026-11-17T09:00"))
        assertEquals(listOf("Manhã", "Tarde"), WeekPlan.days(monday, listOf(a, b))[1].blocks.map { it.entry.title })
    }

    @Test fun usesOfResourcesJoinTheWeekUnlessReturnedOrCancelled() {
        fun use(status: String, start: String = "2026-11-18T10:00", end: String = "2026-11-18T12:00") = ResourceUseItem(
            uid = "r-$status", resourceUid = "res", resourceName = "Sala 2", quantity = 1, who = "", sector = "", purpose = "",
            start = start, end = end, returnedAt = "", status = status, statusLabel = status, notes = "", overdue = false
        )
        val reserved = WeekPlan.use(use("reservado"))
        assertNotNull(reserved)
        assertEquals(WeekKind.USE, reserved!!.kind)
        assertEquals("10:00–12:00", WeekPlan.label(WeekPlan.days(monday, listOf(reserved))[2].blocks.single()))
        assertNotNull(WeekPlan.use(use("em_uso")))
        assertNull(WeekPlan.use(use("devolvido")))
        assertNull(WeekPlan.use(use("cancelado")))
        assertNull(WeekPlan.use(use("reservado", start = "ontem")))
        assertNotEquals(null, WeekPlan.use(use("reservado", end = "")))
    }

    @Test fun theTitleOfTheWeekDay() {
        assertEquals("ter 17/11", weekDayTitle(LocalDate.of(2026, 11, 17)))
        assertEquals("dom 22/11", weekDayTitle(LocalDate.of(2026, 11, 22)))
    }
}
