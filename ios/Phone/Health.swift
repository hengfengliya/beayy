import HealthKit
import UIKit

/// 苹果健康：只读，SDK 里能读的类型全读（不读的见 数据源/数据源_20261029_苹果健康.md）。
/// 每种类型一个增量查询（锚点存在 Application Support/health/anchors/），新数据按开始时刻的本地日归进一天一个文件
/// （health/days/<日>.jsonl.z，压缩的 JSON 行），有改动的那几天各传一整块 health.<日>，格式同停留点那样整天替换。
/// - 累加的类型（步数、能量、距离、锻炼分钟……）不存原始样本：iPhone 和手表各记一份会重复，存苹果去重后的每小时合计（kind＝hour）；
/// - 其他（心率、睡眠、体重、锻炼、心情、心电图……）存原始样本（kind＝sample），每条一行，id＝样本 UUID。
/// 苹果在新数据进来时后台拉起 app（每种类型最多每小时一次），手机锁着时健康数据库读不了，等解锁后再读
final class Health: ObservableObject {
    static let shared = Health()

    @Published private(set) var running = false
    @Published private(set) var lastRun: Date? = UserDefaults.standard.object(forKey: "healthLastRun") as? Date
    @Published private(set) var lastError: String?
    @Published private(set) var pendingDays = 0

    private let store = HKHealthStore()
    private let lock = NSLock()
    /// 后台拉起时系统给的完成回调：这一轮读完再一起回
    private var waiting: [() -> Void] = []
    private var started = false

    var available: Bool { HKHealthStore.isHealthDataAvailable() }
    /// 问过授权没有（苹果不告诉 app 读权限给没给，没给的类型读出来是空的）
    var asked: Bool { UserDefaults.standard.bool(forKey: "healthAsked") }

    // MARK: 类型（iOS 26 SDK 的 HKTypeIdentifiers.h；NikeFuel 不读）

    private static let quantityIDs = """
    AppleSleepingWristTemperature BodyFatPercentage BodyMass BodyMassIndex ElectrodermalActivity Height LeanBodyMass WaistCircumference \
    ActiveEnergyBurned AppleExerciseTime AppleMoveTime AppleStandTime BasalEnergyBurned CrossCountrySkiingSpeed CyclingCadence \
    CyclingFunctionalThresholdPower CyclingPower CyclingSpeed DistanceCrossCountrySkiing DistanceCycling DistanceDownhillSnowSports \
    DistancePaddleSports DistanceRowing DistanceSkatingSports DistanceSwimming DistanceWalkingRunning DistanceWheelchair \
    EstimatedWorkoutEffortScore FlightsClimbed PaddleSportsSpeed PhysicalEffort PushCount RowingSpeed RunningPower RunningSpeed \
    StepCount SwimmingStrokeCount UnderwaterDepth WorkoutEffortScore EnvironmentalAudioExposure EnvironmentalSoundReduction \
    HeadphoneAudioExposure AtrialFibrillationBurden HeartRate HeartRateRecoveryOneMinute HeartRateVariabilitySDNN \
    PeripheralPerfusionIndex RestingHeartRate VO2Max WalkingHeartRateAverage AppleWalkingSteadiness RunningGroundContactTime \
    RunningStrideLength RunningVerticalOscillation SixMinuteWalkTestDistance StairAscentSpeed StairDescentSpeed \
    WalkingAsymmetryPercentage WalkingDoubleSupportPercentage WalkingSpeed WalkingStepLength DietaryBiotin DietaryCaffeine \
    DietaryCalcium DietaryCarbohydrates DietaryChloride DietaryCholesterol DietaryChromium DietaryCopper DietaryEnergyConsumed \
    DietaryFatMonounsaturated DietaryFatPolyunsaturated DietaryFatSaturated DietaryFatTotal DietaryFiber DietaryFolate DietaryIodine \
    DietaryIron DietaryMagnesium DietaryManganese DietaryMolybdenum DietaryNiacin DietaryPantothenicAcid DietaryPhosphorus \
    DietaryPotassium DietaryProtein DietaryRiboflavin DietarySelenium DietarySodium DietarySugar DietaryThiamin DietaryVitaminA \
    DietaryVitaminB12 DietaryVitaminB6 DietaryVitaminC DietaryVitaminD DietaryVitaminE DietaryVitaminK DietaryWater DietaryZinc \
    BloodAlcoholContent BloodPressureDiastolic BloodPressureSystolic InsulinDelivery NumberOfAlcoholicBeverages NumberOfTimesFallen \
    TimeInDaylight UVExposure WaterTemperature BasalBodyTemperature AppleSleepingBreathingDisturbances ForcedExpiratoryVolume1 \
    ForcedVitalCapacity InhalerUsage OxygenSaturation PeakExpiratoryFlowRate RespiratoryRate BloodGlucose BodyTemperature
    """.split(separator: " ").map(String.init)

    private static let categoryIDs = """
    AppleStandHour EnvironmentalAudioExposureEvent HeadphoneAudioExposureEvent HighHeartRateEvent HypertensionEvent \
    IrregularHeartRhythmEvent LowCardioFitnessEvent LowHeartRateEvent MindfulSession AppleWalkingSteadinessEvent HandwashingEvent \
    ToothbrushingEvent BleedingAfterPregnancy BleedingDuringPregnancy CervicalMucusQuality Contraceptive InfrequentMenstrualCycles \
    IntermenstrualBleeding IrregularMenstrualCycles Lactation MenstrualFlow OvulationTestResult PersistentIntermenstrualBleeding \
    Pregnancy PregnancyTestResult ProgesteroneTestResult ProlongedMenstrualPeriods SexualActivity SleepApneaEvent SleepAnalysis \
    AbdominalCramps Acne AppetiteChanges BladderIncontinence Bloating BreastPain ChestTightnessOrPain Chills Constipation Coughing \
    Diarrhea Dizziness DrySkin Fainting Fatigue Fever GeneralizedBodyAche HairLoss Headache Heartburn HotFlashes LossOfSmell \
    LossOfTaste LowerBackPain MemoryLapse MoodChanges Nausea NightSweats PelvicPain RapidPoundingOrFlutteringHeartbeat RunnyNose \
    ShortnessOfBreath SinusCongestion SkippedHeartbeat SleepChanges SoreThroat VaginalDryness Vomiting Wheezing
    """.split(separator: " ").map(String.init)

    static let types: [HKSampleType] = {
        var t: [HKSampleType] = quantityIDs.compactMap { HKQuantityType.quantityType(forIdentifier: .init(rawValue: "HKQuantityTypeIdentifier" + $0)) }
        t += categoryIDs.compactMap { HKCategoryType.categoryType(forIdentifier: .init(rawValue: "HKCategoryTypeIdentifier" + $0)) }
        t += [HKObjectType.workoutType(), HKObjectType.electrocardiogramType(), HKObjectType.audiogramSampleType(), HKObjectType.stateOfMindType(),
              HKScoredAssessmentType(.GAD7), HKScoredAssessmentType(.PHQ9)]
        return t
    }()

    /// HKQuantityTypeIdentifierHeartRate → HeartRate，HKDataTypeStateOfMind → StateOfMind；锻炼 → Workout
    static func short(_ t: HKSampleType) -> String {
        let id = t.identifier
        if t is HKWorkoutType { return "Workout" }
        if let r = id.range(of: "Identifier") { return String(id[r.upperBound...]) }
        return id.hasPrefix("HKDataType") ? String(id.dropFirst("HKDataType".count)) : id
    }

    private static func cumulative(_ t: HKSampleType) -> HKQuantityType? {
        guard let q = t as? HKQuantityType, q.aggregationStyle == .cumulative else { return nil }
        return q
    }

    // MARK: 授权、后台

    /// 「设置 → 这台手机 → 苹果健康 → 连接」：问一次授权（系统弹窗里用户自己勾），然后开始读
    func connect() async {
        guard available else { return }
        do {
            try await store.requestAuthorization(toShare: [], read: Set(Self.types))
            UserDefaults.standard.set(true, forKey: "healthAsked")
            start()
        } catch {
            await setError(tr("授权没成功：{e}", ["e": error.localizedDescription]))
        }
    }

    /// 启动时（AppDelegate）：问过授权就挂上每种类型的观察者。苹果在后台拉起 app 送新数据时也走这里，所以要尽早建好
    func start() {
        guard available, asked else { return }
        lock.lock()
        let first = !started
        started = true
        lock.unlock()
        guard first else { kick(nil); return }
        for t in Self.types {
            let q = HKObserverQuery(sampleType: t, predicate: nil) { [weak self] _, done, _ in self?.kick(done) }
            store.execute(q)
            store.enableBackgroundDelivery(for: t, frequency: .hourly) { _, _ in }
        }
        refreshPending()
    }

    /// 有新数据（或回到前台）：读一轮；正在读就把完成回调排上，读完一起回
    func kick(_ done: (() -> Void)?) {
        lock.lock()
        if let done { waiting.append(done) }
        let busy = isRunning
        if busy { again = true } else { isRunning = true }
        lock.unlock()
        guard !busy else { return }
        Task {
            let bg = await MainActor.run { UIApplication.shared.beginBackgroundTask(withName: "health") }
            await MainActor.run { self.running = true }
            // 读的时候又来了新数据：读完再读一轮（前面的类型可能已经读过了）
            repeat {
                self.lock.withLock { self.again = false }
                await self.readAll()
            } while self.lock.withLock({ self.again })
            let cbs = self.lock.withLock {
                defer { self.waiting = []; self.isRunning = false }
                return self.waiting
            }
            cbs.forEach { $0() }
            await MainActor.run {
                self.running = false
                UIApplication.shared.endBackgroundTask(bg)
            }
            Uploader.shared.uploadPending()
        }
    }

    private var isRunning = false
    private var again = false

    @MainActor private func setError(_ s: String?) { lastError = s }

    // MARK: 读

    private func readAll() async {
        var units: [HKQuantityType: HKUnit] = [:]
        // 一起取；有一种取不到整批会失败，那就一种一种取
        let qs = Self.types.compactMap { $0 as? HKQuantityType }
        if let u = try? await store.preferredUnits(for: Set(qs)) {
            for (t, unit) in u { units[t] = Self.unit(t, unit) }
        } else {
            for t in qs { units[t] = Self.unit(t, (try? await store.preferredUnits(for: [t]))?[t]) }
        }
        var err: String?
        for t in Self.types {
            do {
                try await readType(t, units: units)
            } catch let e as HKError where e.code == .errorDatabaseInaccessible {
                err = tr("手机锁着时读不了健康数据，解锁后接着读")
                break
            } catch let e as HKError where e.code == .errorAuthorizationNotDetermined || e.code == .errorAuthorizationDenied {
                continue  // 这种类型没给读（或授权弹窗里没有它）：跳过，不算错
            } catch {
                err = "\(Self.short(t))：\(error.localizedDescription)"
            }
        }
        do {
            try await hourly()
        } catch {
            err = err ?? tr("每小时合计：{e}", ["e": error.localizedDescription])
        }
        let now = Date(), msg = err
        UserDefaults.standard.set(now, forKey: "healthLastRun")
        await MainActor.run { self.lastRun = now; self.lastError = msg }
        refreshPending()
    }

    /// 一种类型从上次的锚点读到最新：每批 2000 条（累加类型 10000 条，只用来知道哪几天变了），存好这批再记锚点
    private func readType(_ t: HKSampleType, units: [HKQuantityType: HKUnit]) async throws {
        let cum = Self.cumulative(t)
        let limit = cum == nil ? 2000 : 10000
        while true {
            let q = HKAnchoredObjectQueryDescriptor(predicates: [.sample(type: t)], anchor: anchor(t), limit: limit)
            let r = try await q.result(for: store)
            lock.withLock {
                var st = state()
                var cache: [String: [String: Data]] = [:]
                var changed = Set<String>()
                if let cum {
                    let name = Self.short(cum)
                    var days = Set(st.statsDirty[name] ?? [])
                    for s in r.addedSamples { days.insert(Self.day(s.startDate)) }
                    // 删了的样本不知道是哪天的：近 7 天重算
                    if !r.deletedObjects.isEmpty { days.formUnion(Self.recentDays(7)) }
                    st.statsDirty[name] = days.sorted()
                } else {
                    for s in r.addedSamples {
                        guard let row = Self.row(s, units: units) else { continue }
                        let d = Self.day(s.startDate)
                        if cache[d] == nil { cache[d] = load(d) }
                        cache[d]![s.uuid.uuidString] = row
                        changed.insert(d)
                    }
                    // 删了的：在近 30 天的文件里找这个 id
                    if !r.deletedObjects.isEmpty {
                        let ids = Set(r.deletedObjects.map(\.uuid.uuidString))
                        for d in Self.recentDays(30) {
                            if cache[d] == nil { cache[d] = load(d) }
                            let before = cache[d]!.count
                            cache[d] = cache[d]!.filter { !ids.contains($0.key) }
                            if cache[d]!.count != before { changed.insert(d) }
                        }
                    }
                }
                for d in changed {
                    save(d, cache[d] ?? [:])
                    st.mark(d)
                }
                setState(st)
                saveAnchor(t, r.newAnchor)
            }
            if r.addedSamples.count + r.deletedObjects.count < limit { break }
        }
    }

    /// 累加类型：有变动的那些天，按小时取苹果去重后的合计（一次查连续的最多 31 天），整天替换这种类型的小时行
    private func hourly() async throws {
        let dirty = lock.withLock { state().statsDirty }
        let cal = Calendar.current
        for (name, days) in dirty where !days.isEmpty {
            guard let t = Self.types.compactMap(Self.cumulative).first(where: { Self.short($0) == name }) else { continue }
            let unit = Self.unit(t, (try? await store.preferredUnits(for: [t]))?[t])
            var chunks: [[String]] = []
            for d in days {
                if let last = chunks.last?.last, let a = Self.date(last), let b = Self.date(d),
                   cal.dateComponents([.day], from: a, to: b).day! <= 1, chunks.last!.count < 31 {
                    chunks[chunks.count - 1].append(d)
                } else {
                    chunks.append([d])
                }
            }
            for c in chunks {
                guard let a = Self.date(c.first!), let z = Self.date(c.last!).flatMap({ cal.date(byAdding: .day, value: 1, to: $0) }) else { continue }
                let q = HKStatisticsCollectionQueryDescriptor(
                    predicate: .quantitySample(type: t, predicate: HKQuery.predicateForSamples(withStart: a, end: z)),
                    options: .cumulativeSum, anchorDate: a, intervalComponents: DateComponents(hour: 1))
                let coll = try await q.result(for: store)
                var byDay: [String: [String: Data]] = [:]
                for s in coll.statistics() {
                    guard let q = s.sumQuantity(), q.is(compatibleWith: unit) else { continue }
                    let v = q.doubleValue(for: unit)
                    guard v > 0 else { continue }
                    let row: [String: Any] = ["kind": "hour", "type": name, "start": s.startDate.iso8601UTC, "end": s.endDate.iso8601UTC,
                                              "value": (v * 1000).rounded() / 1000, "unit": unit.unitString]
                    let id = "h:\(name):\(s.startDate.iso8601UTC)"
                    var r = row
                    r["id"] = id
                    byDay[Self.day(s.startDate), default: [:]][id] = try? JSONSerialization.data(withJSONObject: r, options: [.sortedKeys])
                }
                lock.withLock {
                    var st = state()
                    for d in c {
                        var rows = load(d).filter { !$0.key.hasPrefix("h:\(name):") }
                        rows.merge(byDay[d] ?? [:]) { $1 }
                        save(d, rows)
                        st.mark(d)
                    }
                    st.statsDirty[name] = (st.statsDirty[name] ?? []).filter { !c.contains($0) }
                    setState(st)
                }
            }
        }
    }

    /// 用手机设置的单位，但能量统一千卡、距离统一米（免得跟着设置变成千焦、英里）
    private static func unit(_ t: HKQuantityType, _ preferred: HKUnit?) -> HKUnit {
        if t.is(compatibleWith: .kilocalorie()) { return .kilocalorie() }
        if t.is(compatibleWith: .meter()) { return .meter() }
        if let preferred, t.is(compatibleWith: preferred) { return preferred }
        return fallbackUnit(t)
    }

    /// 取不到手机设置的单位时：挑一个对得上的（单位不对 HealthKit 直接抛异常让 app 崩掉）
    private static let fallbacks: [HKUnit] = [
        .count(), .count().unitDivided(by: .minute()), .percent(), .minute(), .second(), .secondUnit(with: .milli), .gram(),
        .gramUnit(with: .kilo), .gramUnit(with: .milli), .liter(), .literUnit(with: .milli), .degreeCelsius(), .millimeterOfMercury(),
        .decibelAWeightedSoundPressureLevel(), .decibelHearingLevel(), .gramUnit(with: .milli).unitDivided(by: .literUnit(with: .deci)),
        .meter().unitDivided(by: .second()), .watt(), .internationalUnit(), .siemen(), .atmosphere(), .hertz(), .volt(), .appleEffortScore(),
        .literUnit(with: .milli).unitDivided(by: .gramUnit(with: .kilo).unitMultiplied(by: .minute())),
        .kilocalorie().unitDivided(by: .hour().unitMultiplied(by: .gramUnit(with: .kilo))),
    ]

    private static func fallbackUnit(_ t: HKQuantityType) -> HKUnit {
        fallbacks.first { t.is(compatibleWith: $0) } ?? .count()
    }

    /// 一条样本 → 一行
    private static func row(_ s: HKSample, units: [HKQuantityType: HKUnit]) -> Data? {
        var r: [String: Any] = ["id": s.uuid.uuidString, "kind": "sample", "type": short(s.sampleType),
                                "start": s.startDate.iso8601UTC, "end": s.endDate.iso8601UTC,
                                "source": s.sourceRevision.source.name, "bundle": s.sourceRevision.source.bundleIdentifier]
        if let d = s.device?.model ?? s.device?.name { r["device"] = d }
        if let m = s.metadata, !m.isEmpty { r["meta"] = meta(m) }
        switch s {
        case let q as HKQuantitySample:
            let u = units[q.quantityType] ?? fallbackUnit(q.quantityType)
            if q.quantity.is(compatibleWith: u) {
                r["value"] = q.quantity.doubleValue(for: u)
                r["unit"] = u.unitString
            } else {
                r["text"] = q.quantity.description
            }
        case let c as HKCategorySample:
            r["value"] = c.value
        case let w as HKWorkout:
            r["activity"] = w.workoutActivityType.rawValue
            r["duration"] = w.duration
            var stats: [String: Any] = [:]
            for (t, st) in w.allStatistics {
                let u = units[t] ?? fallbackUnit(t)
                guard t.is(compatibleWith: u) else { continue }
                var x: [String: Any] = ["unit": u.unitString]
                if let v = st.sumQuantity() { x["sum"] = v.doubleValue(for: u) }
                if let v = st.averageQuantity() { x["avg"] = v.doubleValue(for: u) }
                if let v = st.minimumQuantity() { x["min"] = v.doubleValue(for: u) }
                if let v = st.maximumQuantity() { x["max"] = v.doubleValue(for: u) }
                stats[short(t)] = x
            }
            if !stats.isEmpty { r["stats"] = stats }
        case let m as HKStateOfMind:
            r["value"] = m.valence
            r["mind_kind"] = m.kind.rawValue
            r["labels"] = m.labels.map(\.rawValue)
            r["associations"] = m.associations.map(\.rawValue)
        case let e as HKElectrocardiogram:
            r["value"] = e.classification.rawValue
            if let hr = e.averageHeartRate { r["avg_hr"] = hr.doubleValue(for: .count().unitDivided(by: .minute())) }
            r["symptoms"] = e.symptomsStatus.rawValue
        case let a as HKScoredAssessment:
            r["value"] = a.score
        default:
            break
        }
        return try? JSONSerialization.data(withJSONObject: r, options: [.sortedKeys])
    }

    /// 元数据：键去掉 HKMetadataKey 前缀，值转成 JSON 能放的
    private static func meta(_ m: [String: Any]) -> [String: Any] {
        var out: [String: Any] = [:]
        for (k, v) in m {
            let key = k.hasPrefix("HKMetadataKey") ? String(k.dropFirst("HKMetadataKey".count)) : k
            switch v {
            case let s as String: out[key] = s
            case let n as NSNumber: out[key] = n
            case let d as Date: out[key] = d.iso8601UTC
            case let q as HKQuantity: out[key] = q.description
            default: out[key] = String(describing: v)
            }
        }
        return out
    }

    // MARK: 存

    private static var dir: URL {
        let d = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("health", isDirectory: true)
        for sub in ["days", "anchors"] {
            try? FileManager.default.createDirectory(at: d.appendingPathComponent(sub), withIntermediateDirectories: true)
        }
        return d
    }

    private func anchor(_ t: HKSampleType) -> HKQueryAnchor? {
        let f = Self.dir.appendingPathComponent("anchors/\(Self.short(t)).anchor")
        guard let d = try? Data(contentsOf: f) else { return nil }
        return try? NSKeyedUnarchiver.unarchivedObject(ofClass: HKQueryAnchor.self, from: d)
    }

    private func saveAnchor(_ t: HKSampleType, _ a: HKQueryAnchor) {
        guard let d = try? NSKeyedArchiver.archivedData(withRootObject: a, requiringSecureCoding: true) else { return }
        try? d.write(to: Self.dir.appendingPathComponent("anchors/\(Self.short(t)).anchor"), options: .atomic)
    }

    private func file(_ day: String) -> URL { Self.dir.appendingPathComponent("days/\(day).jsonl.z") }

    /// 一天的行：id → 一行 JSON
    private func load(_ day: String) -> [String: Data] {
        guard let z = try? Data(contentsOf: file(day)), let d = try? (z as NSData).decompressed(using: .zlib) as Data else { return [:] }
        var out: [String: Data] = [:]
        for line in d.split(separator: 0x0a) where !line.isEmpty {
            if let o = try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any], let id = o["id"] as? String { out[id] = Data(line) }
        }
        return out
    }

    private func save(_ day: String, _ rows: [String: Data]) {
        var d = Data()
        for k in rows.keys.sorted() {
            d.append(rows[k]!)
            d.append(0x0a)
        }
        guard let z = try? (d as NSData).compressed(using: .zlib) as Data else { return }
        try? z.write(to: file(day), options: .atomic)
    }

    /// 有改动、还没传的天（值＝改动的序号，传完时对得上才算传完）；累加类型还没重算小时合计的天
    private struct State: Codable {
        var pending: [String: Int] = [:]
        var seq = 0
        var statsDirty: [String: [String]] = [:]
        mutating func mark(_ day: String) { seq += 1; pending[day] = seq }
    }

    private func state() -> State {
        UserDefaults.standard.data(forKey: "healthState").flatMap { try? JSONDecoder().decode(State.self, from: $0) } ?? State()
    }

    private func setState(_ s: State) {
        UserDefaults.standard.set(try? JSONEncoder().encode(s), forKey: "healthState")
    }

    private func refreshPending() {
        lock.lock()
        let n = state().pending.count
        lock.unlock()
        DispatchQueue.main.async { self.pendingDays = n }
    }

    // MARK: 给上传用

    /// 待传的天，新的在前，最多 n 天（补历史时一次排太多后台任务，传完一批再排下一批）
    func pendingUploads(_ n: Int) -> [(day: String, seq: Int)] {
        lock.lock()
        defer { lock.unlock() }
        return state().pending.sorted { $0.key > $1.key }.prefix(n).map { ($0.key, $0.value) }
    }

    /// 一天的明文：每行一条，按 id 排
    func dayRows(_ day: String) -> Data {
        lock.lock()
        defer { lock.unlock() }
        var d = Data()
        let rows = load(day)
        for k in rows.keys.sorted() {
            d.append(rows[k]!)
            d.append(0x0a)
        }
        return d
    }

    /// 传完了：传的时候这天没再改才算
    func uploaded(_ day: String, seq: Int) {
        lock.lock()
        var st = state()
        if st.pending[day] == seq { st.pending[day] = nil }
        setState(st)
        lock.unlock()
        refreshPending()
    }

    // MARK: 日期

    private static func dayFormatter() -> DateFormatter {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.calendar = Calendar(identifier: .gregorian)
        f.timeZone = .current
        f.dateFormat = "yyyy-MM-dd"
        return f
    }

    static func day(_ d: Date) -> String { dayFormatter().string(from: d) }
    static func date(_ s: String) -> Date? { dayFormatter().date(from: s) }
    static func recentDays(_ n: Int) -> [String] {
        (0..<n).compactMap { Calendar.current.date(byAdding: .day, value: -$0, to: Date()).map(day) }
    }
}

#if targetEnvironment(simulator)
extension Health {
    /// 模拟器里造一天的演示数据（昨晚睡眠、今天心率、步数、一次跑步、一条心情），看采集、界面用；模拟器不上传
    func seedDemo() async {
        let hr = HKQuantityType(.heartRate), steps = HKQuantityType(.stepCount), energy = HKQuantityType(.activeEnergyBurned)
        let sleep = HKCategoryType(.sleepAnalysis)
        do {
            try await store.requestAuthorization(toShare: [hr, steps, energy, sleep, HKObjectType.workoutType(), HKObjectType.stateOfMindType()], read: Set(Self.types))
            UserDefaults.standard.set(true, forKey: "healthAsked")
            let cal = Calendar.current, d0 = cal.startOfDay(for: Date())
            let at = { (h: Double) in d0.addingTimeInterval(h * 3600) }
            var objs: [HKObject] = []
            // 昨晚 23:40 上床，00:05 睡着，07:10 醒：浅睡、深睡、快速眼动轮着来，中间醒一次
            objs.append(HKCategorySample(type: sleep, value: HKCategoryValueSleepAnalysis.inBed.rawValue, start: at(-0.33), end: at(7.25)))
            let stages: [(HKCategoryValueSleepAnalysis, Double)] = [(.asleepCore, 1.2), (.asleepDeep, 0.8), (.asleepREM, 0.4), (.asleepCore, 1.3), (.awake, 0.1),
                                                                     (.asleepDeep, 0.6), (.asleepREM, 0.6), (.asleepCore, 1.5), (.asleepREM, 0.5)]
            var t = 0.08
            for (v, len) in stages {
                objs.append(HKCategorySample(type: sleep, value: v.rawValue, start: at(t), end: at(t + len)))
                t += len
            }
            let now = Date().timeIntervalSince(d0) / 3600
            for h in stride(from: 7.5, to: min(now, 23.5), by: 1.0 / 6) {
                let bpm = 62 + 14 * sin(h / 3) + (h >= 18.5 && h < 19.2 ? 70 : 0)
                objs.append(HKQuantitySample(type: hr, quantity: HKQuantity(unit: .count().unitDivided(by: .minute()), doubleValue: bpm.rounded()), start: at(h), end: at(h)))
            }
            for h in stride(from: 8.0, to: min(now, 23), by: 1) {
                let n = h >= 18 && h < 19 ? 5200.0 : Double([320, 1100, 240, 80, 600, 150, 900, 210, 60, 480][Int(h) % 10])
                objs.append(HKQuantitySample(type: steps, quantity: HKQuantity(unit: .count(), doubleValue: n), start: at(h), end: at(h + 0.9)))
                objs.append(HKQuantitySample(type: energy, quantity: HKQuantity(unit: .kilocalorie(), doubleValue: n / 22), start: at(h), end: at(h + 0.9)))
            }
            if now > 19.2 {
                objs.append(HKWorkout(activityType: .running, start: at(18.5), end: at(19.2), duration: 0.7 * 3600,
                                      totalEnergyBurned: HKQuantity(unit: .kilocalorie(), doubleValue: 410),
                                      totalDistance: HKQuantity(unit: .meter(), doubleValue: 6200), metadata: nil))
            }
            objs.append(HKStateOfMind(date: at(min(now, 21)), kind: .dailyMood, valence: 0.4, labels: [.content, .calm], associations: [.work, .fitness]))
            try await store.save(objs)
            start()
            kick(nil)
        } catch {
            await setError(tr("造演示数据没成功：{e}", ["e": error.localizedDescription]))
        }
    }
}
#endif
