// Guided tour entry point (HS2-DHCY18). The workspace sorts views in definition
// order, so this view is first in the list and opens by default.

custom "HS2-00-Overview" "0 · Overview: how Hot Sheet fits together" "Start here. Double-click any box to open its detailed view; use the animation controls to build the picture step by step." {
    include ovPeople ovAi ovApp ovServer ovHeadless ovCore ovOrch ovGitStore ovTrackers ovPeers
    animation {
        ovPeople ovApp
        ovServer
        ovCore ovGitStore ovTrackers
        ovHeadless ovAi
        ovOrch ovPeers
    }
    autoLayout tb 200 140
    default
}
