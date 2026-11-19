package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZoneOffset

private val SP: ZoneId = ZoneId.of("America/Sao_Paulo")
private const val CAL = 7L

private fun utcDay(date: String): Long = LocalDate.parse(date).atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli()
private fun at(moment: String): Long = LocalDateTime.parse(moment).atZone(SP).toInstant().toEpochMilli()

private class FakeCalendar : CalendarGateway {
    val events = LinkedHashMap<Long, CalEvent>()
    val reminders = HashMap<Long, List<Int>>()
    var next = 100L
    var writes = 0

    override fun events(calendarId: Long) = events.values.toList()
    override fun insert(calendarId: Long, want: CalWant): Long {
        val id = next++
        events[id] = CalEvent(id, want.title, want.description!!, want.allDay, want.start, want.end)
        reminders[id] = want.reminders
        writes++
        return id
    }
    override fun update(eventId: Long, want: CalWant) {
        check(eventId in events)
        events[eventId] = CalEvent(eventId, want.title, want.description ?: events[eventId]!!.description, want.allDay, want.start, want.end)
        reminders[eventId] = want.reminders
        writes++
    }
    override fun delete(eventId: Long) {
        events.remove(eventId)
        writes++
    }

    fun find(uid: String, kind: CalKind) = events.values.single { CalendarRules.keyOf(it.description) == CalKey(uid, kind) }
    fun findOrNull(uid: String, kind: CalKind) = events.values.singleOrNull { CalendarRules.keyOf(it.description) == CalKey(uid, kind) }

    /** What the person does in the calendar app. */
    fun edit(e: CalEvent, change: (CalEvent) -> CalEvent) { events[e.id] = change(e) }
}

private class FakeTasks : CalTasks {
    val tasks = ArrayList<CalTask>()
    var refuse = false
    var saves = 0
    override fun list() = tasks.toList()
    override fun save(id: Long, fields: Map<String, String>) {
        if (refuse) throw IllegalStateException("refused")
        val i = tasks.indexOfFirst { it.id == id }
        var t = tasks[i]
        fields["title"]?.let { t = t.copy(title = it) }
        fields["due_date"]?.let { t = t.copy(due = it) }
        fields["scheduled_start"]?.let { t = t.copy(start = it) }
        fields["scheduled_end"]?.let { t = t.copy(end = it) }
        tasks[i] = t
        saves++
    }
    fun get(uid: String) = tasks.single { it.uid == uid }
    fun change(uid: String, f: (CalTask) -> CalTask) { val i = tasks.indexOfFirst { it.uid == uid }; tasks[i] = f(tasks[i]) }
}

private class MemoryStore : CalStore {
    var snapshot: CalSnapshot? = null
    override fun load() = snapshot
    override fun save(snapshot: CalSnapshot?) { this.snapshot = snapshot }
}

private fun task(
    uid: String = "u1", id: Long = 1, title: String = "Responder o ofício 123", due: String = "2026-11-30",
    start: String = "", end: String = "", reminders: List<Int> = emptyList(), doc: String = "OF-123", done: Boolean = false
) = CalTask(uid, id, title, done, due, start, end, reminders, doc)

private class World(vararg initial: CalTask) {
    val cal = FakeCalendar()
    val tasks = FakeTasks().also { it.tasks.addAll(initial) }
    val store = MemoryStore()
    val engine = CalendarEngine(cal, tasks, store) { SP }
    fun run(calendar: Long = CAL) = engine.run(calendar)
    fun plan() = CalendarRules.reconcile(tasks.list(), cal.events(CAL), store.load() ?: CalSnapshot(CAL, emptyMap()), SP)
}

class AgendaSyncTest {
    @Test fun createsAnAllDayDeadlineAndATimedPlannedEvent() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00", reminders = listOf(10080, 0, 1440)))
        val r = w.run()
        assertTrue(r.warnings.isEmpty())
        val d = w.cal.find("u1", CalKind.DEADLINE)
        assertEquals("Prazo: Responder o ofício 123", d.title)
        assertTrue(d.allDay)
        assertEquals(utcDay("2026-11-30"), d.start)
        assertEquals(utcDay("2026-12-01"), d.end)
        assertEquals("Documento: OF-123\nponte-tarefa:u1:prazo", d.description)
        assertEquals(emptyList<Int>(), w.cal.reminders[d.id])
        val p = w.cal.find("u1", CalKind.PLAN)
        assertEquals("Responder o ofício 123", p.title)
        assertFalse(p.allDay)
        assertEquals(at("2026-11-25T09:00"), p.start)
        assertEquals(at("2026-11-25T18:00"), p.end)
        assertTrue(p.description.endsWith("ponte-tarefa:u1:plano"))
        assertEquals(listOf(0, 1440, 10080), w.cal.reminders[p.id])
    }

    @Test fun aTaskWithoutDocumentHasOnlyTheMarkerInTheDescription() {
        val w = World(task(doc = ""))
        w.run()
        assertEquals("ponte-tarefa:u1:prazo", w.cal.find("u1", CalKind.DEADLINE).description)
    }

    @Test fun anUndatedTaskAndAProjectedOneGetNoEvent() {
        // No deadline, and no fixed start and end (a projected period is not in the task's own fields).
        val w = World(task(due = ""), task(uid = "u2", id = 2, due = "", start = "2026-11-25T09:00", end = ""))
        w.run()
        assertTrue(w.cal.events.isEmpty())
    }

    @Test fun taskChangesGoToTheEvents() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        w.tasks.change("u1") { it.copy(title = "Outro título", due = "2026-12-05", start = "2026-11-26T10:00", end = "2026-11-26T12:00", reminders = listOf(1440)) }
        val r = w.run()
        assertTrue(r.warnings.isEmpty())
        assertFalse(r.tasksChanged)
        val d = w.cal.find("u1", CalKind.DEADLINE)
        assertEquals("Prazo: Outro título", d.title)
        assertEquals(utcDay("2026-12-05"), d.start)
        val p = w.cal.find("u1", CalKind.PLAN)
        assertEquals("Outro título", p.title)
        assertEquals(at("2026-11-26T10:00"), p.start)
        assertEquals(at("2026-11-26T12:00"), p.end)
        assertEquals(listOf(1440), w.cal.reminders[p.id])
    }

    @Test fun aDoneTaskTakesItsEventsOutAndReopeningBringsThemBack() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        assertEquals(2, w.cal.events.size)
        w.tasks.change("u1") { it.copy(done = true) }
        w.run()
        assertTrue(w.cal.events.isEmpty())
        w.tasks.change("u1") { it.copy(done = false) }
        w.run()
        assertEquals(2, w.cal.events.size)
    }

    @Test fun aTaskThatLostItsDeadlineAndATaskGoneFromTheListLoseTheirEvents() {
        val w = World(task(), task(uid = "u2", id = 2, title = "Outra"))
        w.run()
        assertEquals(2, w.cal.events.size)
        w.tasks.change("u1") { it.copy(due = "") }
        w.tasks.tasks.removeAll { it.uid == "u2" } // deleted on another device
        w.run()
        assertTrue(w.cal.events.isEmpty())
    }

    @Test fun movingTheDeadlineEventMovesTheTask() {
        val w = World(task())
        w.run()
        w.cal.edit(w.cal.find("u1", CalKind.DEADLINE)) { it.copy(start = utcDay("2026-12-10"), end = utcDay("2026-12-11")) }
        val r = w.run()
        assertTrue(r.tasksChanged)
        assertTrue(r.warnings.isEmpty())
        assertEquals("2026-12-10", w.tasks.get("u1").due)
        assertEquals(utcDay("2026-12-10"), w.cal.find("u1", CalKind.DEADLINE).start)
    }

    @Test fun aDeadlineEventMadeTimedKeepsItsDayAndGoesBackToAllDay() {
        val w = World(task())
        w.run()
        w.cal.edit(w.cal.find("u1", CalKind.DEADLINE)) { it.copy(allDay = false, start = at("2026-12-02T14:00"), end = at("2026-12-02T15:00")) }
        w.run()
        assertEquals("2026-12-02", w.tasks.get("u1").due)
        val d = w.cal.find("u1", CalKind.DEADLINE)
        assertTrue(d.allDay)
        assertEquals(utcDay("2026-12-02"), d.start)
    }

    @Test fun movingAndResizingThePlannedEventChangesThePeriod() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        w.cal.edit(w.cal.find("u1", CalKind.PLAN)) { it.copy(start = at("2026-11-27T08:30"), end = at("2026-11-28T17:00")) }
        val r = w.run()
        assertTrue(r.tasksChanged)
        assertEquals("2026-11-27T08:30", w.tasks.get("u1").start)
        assertEquals("2026-11-28T17:00", w.tasks.get("u1").end)
        assertEquals("2026-11-30", w.tasks.get("u1").due) // the deadline does not move with the plan
    }

    @Test fun resizingOnlyTheEndKeepsTheStart() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        w.cal.edit(w.cal.find("u1", CalKind.PLAN)) { it.copy(end = at("2026-11-26T12:00")) }
        w.run()
        assertEquals("2026-11-25T09:00", w.tasks.get("u1").start)
        assertEquals("2026-11-26T12:00", w.tasks.get("u1").end)
    }

    @Test fun aPlannedEventMadeAllDayTakesTheDatesAndTheDefaultHours() {
        val w = World(task(start = "2026-11-25T10:00", end = "2026-11-25T11:00"))
        w.run()
        // all-day on the 26th and the 27th (the end is the next midnight)
        w.cal.edit(w.cal.find("u1", CalKind.PLAN)) { it.copy(allDay = true, start = utcDay("2026-11-26"), end = utcDay("2026-11-28")) }
        w.run()
        assertEquals("2026-11-26T09:00", w.tasks.get("u1").start)
        assertEquals("2026-11-27T18:00", w.tasks.get("u1").end)
        val p = w.cal.find("u1", CalKind.PLAN)
        assertFalse(p.allDay)
        assertEquals(at("2026-11-26T09:00"), p.start)
    }

    @Test fun aTitleEditedInTheCalendarRenamesTheTaskAndTheOtherEvent() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        w.cal.edit(w.cal.find("u1", CalKind.DEADLINE)) { it.copy(title = "Prazo: Novo nome") }
        val r = w.run()
        assertTrue(r.tasksChanged)
        assertEquals("Novo nome", w.tasks.get("u1").title)
        assertEquals("Prazo: Novo nome", w.cal.find("u1", CalKind.DEADLINE).title)
        assertEquals("Novo nome", w.cal.find("u1", CalKind.PLAN).title)
        assertTrue(w.plan().isEmpty)
    }

    @Test fun aDeadlineTitleTypedWithoutThePrefixStillRenamesTheTask() {
        val w = World(task())
        w.run()
        w.cal.edit(w.cal.find("u1", CalKind.DEADLINE)) { it.copy(title = "Sem prefixo") }
        w.run()
        assertEquals("Sem prefixo", w.tasks.get("u1").title)
        assertEquals("Prazo: Sem prefixo", w.cal.find("u1", CalKind.DEADLINE).title)
    }

    @Test fun whenBothChangedTheTaskWinsAndAWarningIsRecorded() {
        val w = World(task())
        w.run()
        w.tasks.change("u1") { it.copy(due = "2026-12-20") }
        w.cal.edit(w.cal.find("u1", CalKind.DEADLINE)) { it.copy(start = utcDay("2026-12-10"), end = utcDay("2026-12-11")) }
        val r = w.run()
        assertFalse(r.tasksChanged)
        assertEquals("2026-12-20", w.tasks.get("u1").due)
        assertEquals(utcDay("2026-12-20"), w.cal.find("u1", CalKind.DEADLINE).start)
        assertEquals(
            listOf("A mudança feita na agenda em \"Responder o ofício 123\" foi desfeita porque a tarefa também mudou."),
            r.warnings
        )
    }

    @Test fun bothChangedToTheSameValueIsNoConflict() {
        val w = World(task())
        w.run()
        w.tasks.change("u1") { it.copy(due = "2026-12-20") }
        w.cal.edit(w.cal.find("u1", CalKind.DEADLINE)) { it.copy(start = utcDay("2026-12-20"), end = utcDay("2026-12-21")) }
        val r = w.run()
        assertTrue(r.warnings.isEmpty())
        assertEquals("2026-12-20", w.tasks.get("u1").due)
    }

    @Test fun anEditInOneFieldAndATaskChangeInAnotherBothStand() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        w.tasks.change("u1") { it.copy(start = "2026-11-24T09:00") } // the task moved the start
        w.cal.edit(w.cal.find("u1", CalKind.PLAN)) { it.copy(end = at("2026-11-26T12:00")) } // the calendar moved the end
        val r = w.run()
        assertTrue(r.warnings.isEmpty())
        assertEquals("2026-11-24T09:00", w.tasks.get("u1").start)
        assertEquals("2026-11-26T12:00", w.tasks.get("u1").end)
        val p = w.cal.find("u1", CalKind.PLAN)
        assertEquals(at("2026-11-24T09:00"), p.start)
        assertEquals(at("2026-11-26T12:00"), p.end)
    }

    @Test fun anEventDeletedInTheCalendarIsRecreatedAndTheTaskStays() {
        val w = World(task())
        w.run()
        val old = w.cal.find("u1", CalKind.DEADLINE)
        w.cal.events.remove(old.id)
        val r = w.run()
        assertFalse(r.tasksChanged)
        assertEquals("2026-11-30", w.tasks.get("u1").due)
        val back = w.cal.find("u1", CalKind.DEADLINE)
        assertTrue(back.id != old.id)
        assertEquals(old.start, back.start)
    }

    @Test fun eventsTheUserCreatedInThatCalendarAreIgnored() {
        val w = World(task())
        w.run()
        w.cal.events[500] = CalEvent(500, "Dentista", "", false, at("2026-11-20T10:00"), at("2026-11-20T11:00"))
        w.run()
        assertEquals("Dentista", w.cal.events[500]!!.title)
        assertEquals(2, w.cal.events.size)
    }

    @Test fun aLostSnapshotAdoptsTheLowestEventAndLeavesCopiesItDoesNotHold() {
        val w = World(task())
        w.run()
        val first = w.cal.find("u1", CalKind.DEADLINE)
        // another phone made its own copies, and this phone lost its memory
        w.cal.events[300] = first.copy(id = 300)
        w.cal.events[301] = first.copy(id = 301, title = "Prazo: velho")
        w.store.snapshot = null
        val r = w.run()
        assertTrue(r.warnings.isEmpty())
        assertEquals(3, w.cal.events.size) // nothing this phone does not hold is deleted
        assertEquals(first.id, w.store.snapshot!!.marks[CalKey("u1", CalKind.DEADLINE)]!!.eventId) // the lowest id
        assertTrue(w.plan().isEmpty)
        val writes = w.cal.writes
        w.run()
        assertEquals(writes, w.cal.writes) // no delete and recreate loop
    }

    @Test fun anEventOfADoneTaskThatThePhoneDoesNotHoldIsNotDeleted() {
        val w = World(task(done = true))
        w.cal.events[60] = CalEvent(60, "Prazo: x", "ponte-tarefa:u1:prazo", true, utcDay("2026-01-01"), utcDay("2026-01-02"))
        w.run()
        assertNotNull(w.cal.events[60])
    }

    @Test fun aReusedEventIdWithAnotherMarkerIsNotTakenForTheOldTask() {
        val w = World(task(), task(uid = "u2", id = 2, title = "Outra", due = "2026-12-01"))
        w.run()
        val oldU1 = w.cal.find("u1", CalKind.DEADLINE).id
        // the calendar was wiped and refilled: the id of u1's old event now belongs to u2
        w.cal.events.clear()
        w.cal.events[oldU1] = CalEvent(oldU1, "Prazo: Outra", "ponte-tarefa:u2:prazo", true, utcDay("2026-12-01"), utcDay("2026-12-02"))
        w.run()
        assertEquals("Responder o ofício 123", w.tasks.get("u1").title)
        assertEquals("2026-11-30", w.tasks.get("u1").due)
        assertEquals("Outra", w.tasks.get("u2").title)
        assertEquals(oldU1, w.cal.find("u2", CalKind.DEADLINE).id)
        assertEquals("Prazo: Responder o ofício 123", w.cal.find("u1", CalKind.DEADLINE).title)
        assertTrue(w.cal.find("u1", CalKind.DEADLINE).id != oldU1)
    }

    @Test fun anEmptyTaskListWithEventsRememberedTouchesNothing() {
        val w = World(task())
        w.run()
        w.tasks.tasks.clear()
        val writes = w.cal.writes
        w.run()
        assertEquals(writes, w.cal.writes)
        assertEquals(1, w.cal.events.size)
        assertEquals(1, w.store.snapshot!!.marks.size)
    }

    @Test fun durationsAreParsed() {
        assertEquals(86400000L, CalendarRules.parseDuration("P1D"))
        assertEquals(5400000L, CalendarRules.parseDuration("PT1H30M"))
        assertEquals(3600000L, CalendarRules.parseDuration("pt3600s"))
        assertEquals(604800000L + 86400000L, CalendarRules.parseDuration("P1W1D"))
        assertNull(CalendarRules.parseDuration(""))
        assertNull(CalendarRules.parseDuration(null))
        assertNull(CalendarRules.parseDuration("P"))
        assertNull(CalendarRules.parseDuration("PT0S"))
        assertNull(CalendarRules.parseDuration("abc"))
    }

    @Test fun anEventWithoutAUsableEndIsNeverReadAsAnEditNorRewritten() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        val p = w.cal.find("u1", CalKind.PLAN)
        w.cal.edit(p) { it.copy(start = at("2026-11-27T08:30"), end = at("2026-11-27T08:30"), readable = false) }
        val writes = w.cal.writes
        val r = w.run()
        assertFalse(r.tasksChanged)
        assertEquals("2026-11-25T09:00", w.tasks.get("u1").start)
        assertEquals(writes, w.cal.writes)
        assertNotNull(w.store.snapshot!!.marks[CalKey("u1", CalKind.PLAN)])
    }

    @Test fun theUsersNotesInTheDescriptionAreNeverRewritten() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        val d = w.cal.find("u1", CalKind.DEADLINE)
        val notes = "<p>Minhas notas</p><br>ponte-tarefa:u1:prazo<br>"
        w.cal.edit(d) { it.copy(description = notes) }
        val writes = w.cal.writes
        w.run()
        assertEquals(writes, w.cal.writes)
        assertEquals(notes, w.cal.events[d.id]!!.description)
        // a task change updates the event but keeps the notes
        w.tasks.change("u1") { it.copy(due = "2026-12-05") }
        w.run()
        assertEquals(notes, w.cal.events[d.id]!!.description)
        assertEquals(utcDay("2026-12-05"), w.cal.events[d.id]!!.start)
    }

    @Test fun aMissingMarkerIsAppendedAfterTheUsersText() {
        val w = World(task())
        w.run()
        val d = w.cal.find("u1", CalKind.DEADLINE)
        w.cal.edit(d) { it.copy(description = "só minhas notas") }
        w.run()
        assertEquals("só minhas notas\nponte-tarefa:u1:prazo", w.cal.events[d.id]!!.description)
        assertTrue(w.plan().isEmpty)
    }

    @Test fun theMarkerIsFoundInsideHtml() {
        assertEquals(CalKey("abc-1", CalKind.PLAN), CalendarRules.keyOf("<div>Doc</div><br>ponte-tarefa:abc-1:plano</p>"))
        assertEquals(CalKey("abc-1", CalKind.DEADLINE), CalendarRules.keyOf("a\r\nPonte-Tarefa:abc-1:PRAZO\r\n"))
    }

    @Test fun deadlinePrefixesDoNotPileUp() {
        assertEquals("Foo", CalendarRules.stripDeadlinePrefix("Prazo: prazo : PRAZO:Foo"))
        assertEquals("Foo", CalendarRules.stripDeadlinePrefix("  prazo:   Foo "))
        val w = World(task())
        w.run()
        w.cal.edit(w.cal.find("u1", CalKind.DEADLINE)) { it.copy(title = "Prazo: prazo: Novo") }
        w.run()
        assertEquals("Novo", w.tasks.get("u1").title)
        assertEquals("Prazo: Novo", w.cal.find("u1", CalKind.DEADLINE).title)
    }

    @Test fun removingOrMovingOnlyDeletesTheEventsThePhoneHolds() {
        val w = World(task())
        w.run()
        val mine = w.cal.find("u1", CalKind.DEADLINE).id
        w.cal.events[400] = CalEvent(400, "Prazo: de outro", "ponte-tarefa:zzz:prazo", true, utcDay("2026-01-01"), utcDay("2026-01-02"))
        w.run(8L) // another calendar chosen
        assertNull(w.cal.events[mine])
        assertNotNull(w.cal.events[400])
        w.engine.removeEvents(8L)
        assertEquals(listOf(400L), w.cal.events.keys.toList())
    }

    @Test fun anAdoptedEventThatDiffersTakesTheTaskValuesWithoutAWarning() {
        val w = World(task())
        w.cal.events[50] = CalEvent(50, "Prazo: velho", "ponte-tarefa:u1:prazo", true, utcDay("2026-01-01"), utcDay("2026-01-02"))
        val r = w.run()
        assertTrue(r.warnings.isEmpty())
        assertFalse(r.tasksChanged)
        assertEquals(1, w.cal.events.size)
        assertEquals("Prazo: Responder o ofício 123", w.cal.events[50]!!.title)
        assertEquals("2026-11-30", w.tasks.get("u1").due)
    }

    @Test fun markerEventsOfTasksThisPhoneDoesNotKnowAreLeftAlone() {
        val w = World(task())
        w.cal.events[60] = CalEvent(60, "Prazo: de outro", "ponte-tarefa:zzz:prazo", true, utcDay("2026-01-01"), utcDay("2026-01-02"))
        w.run()
        assertNotNull(w.cal.events[60])
    }

    @Test fun aSecondReconcileChangesNothingAndStartsNoLoop() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00", reminders = listOf(0)))
        w.run()
        assertTrue(w.plan().isEmpty)
        val writes = w.cal.writes
        val saves = w.tasks.saves
        val r = w.run()
        assertEquals(writes, w.cal.writes)
        assertEquals(saves, w.tasks.saves)
        assertFalse(r.tasksChanged)
        assertTrue(r.warnings.isEmpty())
    }

    @Test fun afterAnInboundChangeTheNextReconcileIsQuiet() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        w.cal.edit(w.cal.find("u1", CalKind.PLAN)) { it.copy(start = at("2026-11-27T08:30"), end = at("2026-11-28T17:00")) }
        w.cal.edit(w.cal.find("u1", CalKind.DEADLINE)) { it.copy(title = "Prazo: Renomeada") }
        w.run()
        assertTrue(w.plan().isEmpty)
        val writes = w.cal.writes
        val saves = w.tasks.saves
        w.run()
        assertEquals(writes, w.cal.writes)
        assertEquals(saves, w.tasks.saves)
    }

    @Test fun aTaskThePhoneCannotEditGetsItsEventPutBackWithAWarning() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        w.tasks.change("u1") { it.copy(editable = false) }
        w.cal.edit(w.cal.find("u1", CalKind.PLAN)) { it.copy(start = at("2026-11-27T08:30"), end = at("2026-11-28T17:00")) }
        val saves = w.tasks.saves
        val r = w.run()
        assertEquals(saves, w.tasks.saves)
        assertFalse(r.tasksChanged)
        assertEquals("2026-11-25T09:00", w.tasks.get("u1").start)
        assertEquals(at("2026-11-25T09:00"), w.cal.find("u1", CalKind.PLAN).start)
        assertEquals(
            listOf("A mudança feita na agenda em \"Responder o ofício 123\" não pôde ser aplicada à tarefa e foi desfeita."),
            r.warnings
        )
        assertTrue(w.plan().isEmpty)
    }

    @Test fun aChangeTheTaskRefusesIsUndoneInTheCalendar() {
        val w = World(task())
        w.run()
        w.cal.edit(w.cal.find("u1", CalKind.DEADLINE)) { it.copy(start = utcDay("2026-12-10"), end = utcDay("2026-12-11")) }
        w.tasks.refuse = true
        val r = w.run()
        assertEquals("2026-11-30", w.tasks.get("u1").due)
        assertEquals(utcDay("2026-11-30"), w.cal.find("u1", CalKind.DEADLINE).start)
        assertEquals(1, r.warnings.size)
        w.tasks.refuse = false
        assertTrue(w.plan().isEmpty)
    }

    @Test fun anInvalidEditIsUndone() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        // the end before the start
        w.cal.edit(w.cal.find("u1", CalKind.PLAN)) { it.copy(end = at("2026-11-25T08:00")) }
        val r = w.run()
        assertEquals("2026-11-25T18:00", w.tasks.get("u1").end)
        assertEquals(at("2026-11-25T18:00"), w.cal.find("u1", CalKind.PLAN).end)
        assertEquals(1, r.warnings.size)
        // an emptied title
        w.cal.edit(w.cal.find("u1", CalKind.PLAN)) { it.copy(title = "  ") }
        w.run()
        assertEquals("Responder o ofício 123", w.tasks.get("u1").title)
        assertEquals("Responder o ofício 123", w.cal.find("u1", CalKind.PLAN).title)
    }

    @Test fun choosingAnotherCalendarMovesTheEvents() {
        val w = World(task())
        w.run(CAL)
        val old = w.cal.find("u1", CalKind.DEADLINE)
        w.run(8L)
        assertNull(w.cal.events[old.id])
        assertEquals(1, w.cal.events.size)
        assertEquals(8L, w.store.snapshot!!.calendarId)
    }

    @Test fun turningOffCanRemoveTheEvents() {
        val w = World(task(start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        w.cal.events[900] = CalEvent(900, "Dentista", "", false, 1, 2)
        w.engine.removeEvents(CAL)
        assertEquals(listOf(900L), w.cal.events.keys.toList())
        assertNull(w.store.snapshot)
    }

    @Test fun theMarkerIsFoundAnywhereInTheDescription() {
        assertEquals(CalKey("abc-1", CalKind.PLAN), CalendarRules.keyOf("Texto\nponte-tarefa:abc-1:plano\n"))
        assertNull(CalendarRules.keyOf("nada aqui"))
        assertNull(CalendarRules.keyOf("ponte-tarefa:abc:outro"))
    }

    @Test fun aChangeOfTimeZoneMovesTheTimedEventWithoutTouchingTheTask() {
        val w = World(task(due = "", start = "2026-11-25T09:00", end = "2026-11-25T18:00"))
        w.run()
        val utc = ZoneId.of("UTC")
        val plan = CalendarRules.reconcile(w.tasks.list(), w.cal.events(CAL), w.store.load()!!, utc)
        assertTrue(plan.taskChanges.isEmpty())
        assertEquals(1, plan.update.size)
        assertEquals(LocalDateTime.parse("2026-11-25T09:00").atZone(utc).toInstant().toEpochMilli(), plan.update[0].second.start)
    }

    private fun cal(id: Long, name: String, account: String = "eu@gmail.com", type: String = "com.google", owner: String = account, sync: Boolean = true, visible: Boolean = true) =
        CalInfo(id, name, account, type, owner, 0, sync, visible)

    @Test fun holidayAndBirthdayCalendarsAreNotOffered() {
        val list = listOf(
            cal(1, "Ponte"),
            cal(2, "Feriados no Brasil", owner = "pt.brazilian#holiday@group.v.calendar.google.com"),
            cal(3, "Aniversários", owner = "addressbook#contacts@group.v.calendar.google.com")
        )
        assertEquals(listOf(1L), CalendarChoices.offered(list).map { it.id })
    }

    @Test fun calendarsAreGroupedByAccountWithLocalOnesLastAndSortedByName() {
        val list = listOf(
            cal(1, "Trabalho", account = "b@gmail.com"), cal(2, "Ponte", account = "a@gmail.com"),
            cal(3, "Meu celular", account = "Celular", type = "LOCAL"), cal(4, "Casa", account = "b@gmail.com")
        )
        val g = CalendarChoices.grouped(list)
        assertEquals(listOf("a@gmail.com", "b@gmail.com", "Só neste celular"), g.map { it.first })
        assertEquals(listOf("Casa", "Trabalho"), g[1].second.map { it.name })
    }

    @Test fun hiddenOrUnsyncedCalendarsStayInTheListAndSayTheyWillBeSwitchedOn() {
        val hidden = cal(1, "Ponte", visible = false)
        val unsynced = cal(2, "Ponte 2", sync = false)
        val ok = cal(3, "Ok")
        val local = cal(4, "Local", account = "Celular", type = "LOCAL", sync = false, visible = false)
        assertEquals(listOf(1L, 2L, 3L, 4L), CalendarChoices.offered(listOf(hidden, unsynced, ok, local)).map { it.id })
        assertTrue(hidden.needsEnabling && unsynced.needsEnabling)
        assertEquals("não sincronizada — será ligada ao escolher", hidden.note)
        assertEquals("", ok.note)
        assertFalse(ok.needsEnabling)
        assertEquals("só neste celular", local.note)
        assertFalse(local.needsEnabling) // a local calendar has nothing to sync
    }
}
