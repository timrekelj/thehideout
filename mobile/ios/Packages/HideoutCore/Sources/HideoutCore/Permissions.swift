import Foundation

/// What a member may do in a guild (`Permission` in types.proto). The client only uses it to decide
/// which controls to show; the server checks every call.
public struct Permissions: OptionSet, Sendable, Hashable {
    public let rawValue: UInt64
    public init(rawValue: UInt64) { self.rawValue = rawValue }

    public static let administrator = Permissions(rawValue: 1)
    public static let manageServer = Permissions(rawValue: 2)
    public static let manageChannels = Permissions(rawValue: 4)
    public static let manageRoles = Permissions(rawValue: 8)
    public static let manageEmojis = Permissions(rawValue: 16)
    public static let invitePeople = Permissions(rawValue: 32)
    public static let connect = Permissions(rawValue: 64)
    public static let speak = Permissions(rawValue: 128)
    public static let video = Permissions(rawValue: 256)
    public static let muteMembers = Permissions(rawValue: 512)
    public static let moveMembers = Permissions(rawValue: 1024)
    public static let manageMessages = Permissions(rawValue: 2048)
    public static let kickMembers = Permissions(rawValue: 4096)
    public static let banMembers = Permissions(rawValue: 8192)

    /// What every member has without a role: the voice basics.
    public static let baseline: Permissions = [.connect, .speak, .video]
    /// What the owner holds.
    public static let all = Permissions(rawValue: .max)

    /// ADMINISTRATOR implies everything.
    public func allows(_ p: Permissions) -> Bool {
        contains(.administrator) || contains(p)
    }

    /// `me`'s effective permissions: the owner has all, anyone else the baseline plus their roles'.
    public static func effective(guild: Guild, me: ID, myRoles: [ID], roles: [Role]) -> Permissions {
        if guild.ownerID == me { return .all }
        return roles.filter { myRoles.contains($0.id) }.reduce(baseline) { $0.union($1.permissions) }
    }
}
