// swift-tools-version: 5.9
//
// FaberRuntime — Swift runtime package for Faber-generated Swift code.
//
// S6-U1 package identity (faber-target-runtime inventory §7): the recorded
// default is the SwiftPM package at `faber/runtime/swift/`, product/module
// `FaberRuntime`, materialized via core-support. Generated Swift (HIR-Swift
// emit) imports this module instead of inlining the `FaberRuntimeError`
// wrapper surface (S6-U2).
//
// The package has no dependencies; generated projects link it either from a
// checked-out path dependency or from the materialized core-support root.

import PackageDescription

let package = Package(
    name: "FaberRuntime",
    products: [
        .library(name: "FaberRuntime", targets: ["FaberRuntime"])
    ],
    targets: [
        .target(name: "FaberRuntime"),
        .testTarget(
            name: "FaberRuntimeTests",
            dependencies: ["FaberRuntime"]
        ),
    ]
)
