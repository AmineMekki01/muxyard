import CoreGraphics
import Foundation

let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
guard let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
    exit(1)
}

for window in windows {
    let pid = window[kCGWindowOwnerPID as String] as? Int ?? -1
    let owner = window[kCGWindowOwnerName as String] as? String ?? ""
    print("\(pid)\t\(owner)")
}
