import CoreLocation
import SwiftUI

/// 现在的位置（地图页开着时持续定位）：用来校验定位准不准，以及这一刻按记录规则会记成哪个区域（高德，同停留点取名）。
/// 页面开着时持续定位，离开就停；挪动 30 米以上或精度明显变好才重新问高德
final class LiveLocation: NSObject, ObservableObject, CLLocationManagerDelegate {
    @Published var loc: CLLocation?
    @Published var region: Amap.Region?
    @Published var error: String?
    private let manager = CLLocationManager()
    private var asked: CLLocation?

    func start() {
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyBest
        manager.startUpdatingLocation()
    }

    func stop() { manager.stopUpdatingLocation() }

    /// 这一刻会记成的名字（「区 · 区域」）
    var name: String? { region.map { [$0.district, $0.area].compactMap { $0 }.joined(separator: " · ") } }

    func locationManager(_ m: CLLocationManager, didUpdateLocations locs: [CLLocation]) {
        guard let l = locs.last, l.horizontalAccuracy >= 0 else { return }
        DispatchQueue.main.async { self.loc = l; self.error = nil }
        if let p = asked, p.distance(from: l) < 30, l.horizontalAccuracy >= p.horizontalAccuracy * 0.7 { return }
        asked = l
        Task { @MainActor in
            if let r = try? await Amap.region(lat: l.coordinate.latitude, lon: l.coordinate.longitude, acc: l.horizontalAccuracy) { self.region = r }
        }
    }

    func locationManager(_ m: CLLocationManager, didFailWithError error: Error) {
        DispatchQueue.main.async { self.error = tr("取不到位置：{e}", ["e": error.localizedDescription]) }
    }
}
