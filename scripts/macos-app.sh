#!/bin/sh
# Builds target/release/Scrawl.app: the release binary in an app bundle that declares Scrawl's
# document type, so Finder opens .scrawl files with it (double-click, Open With, the Dock icon),
# and Quick Look extensions (macos/quicklook) that show the preview saved in each file as its
# Finder thumbnail and space-bar preview.
# Copy the app to /Applications to make it the default for .scrawl files.
# With --universal, the app runs natively on both Apple silicon and Intel Macs (for releases).
# SIGN_IDENTITY (e.g. "Developer ID Application: ...") signs it for distribution, with the hardened
# runtime notarization requires; without it the app is signed ad hoc, which only runs locally.
set -eu
cd "$(dirname "$0")/.."

if [ -n "${SIGN_IDENTITY:-}" ]; then
	sign() { codesign --force --sign "$SIGN_IDENTITY" --options runtime --timestamp "$@"; }
else
	sign() { codesign --force --sign - "$@"; }
fi

if [ "${1:-}" = --universal ]; then archs="arm64 x86_64"; else archs=$(uname -m); fi
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=target/release/Scrawl.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
bins=
for arch in $archs; do
	triple=$(echo "$arch" | sed 's/arm64/aarch64/')-apple-darwin
	cargo build -p scrawl-app --release --target "$triple"
	bins="$bins target/$triple/release/scrawl"
done
lipo -create $bins -output "$app/Contents/MacOS/scrawl"

# The icon, from assets/icon.svg, at every size macOS asks for.
iconset=target/release/Scrawl.iconset
rm -rf "$iconset" && mkdir -p "$iconset"
cargo build -q -p scrawl-render --release --example rasterize
for size in 16 32 128 256 512; do
	target/release/examples/rasterize assets/icon.svg "$iconset/icon_${size}x${size}.png" $size
	target/release/examples/rasterize assets/icon.svg "$iconset/icon_${size}x${size}@2x.png" $((size * 2))
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Scrawl.icns"
rm -rf "$iconset"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>Scrawl</string>
	<key>CFBundleDisplayName</key><string>Scrawl</string>
	<key>CFBundleIdentifier</key><string>dev.scrawl.Scrawl</string>
	<key>CFBundleExecutable</key><string>scrawl</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>$version</string>
	<key>CFBundleVersion</key><string>$version</string>
	<key>LSMinimumSystemVersion</key><string>11.0</string>
	<key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
	<key>NSHighResolutionCapable</key><true/>
	<key>CFBundleIconFile</key><string>Scrawl</string>
	<key>CFBundleDocumentTypes</key>
	<array>
		<dict>
			<key>CFBundleTypeName</key><string>Scrawl Document</string>
			<key>CFBundleTypeIconFile</key><string>Scrawl</string>
			<key>CFBundleTypeRole</key><string>Editor</string>
			<key>LSHandlerRank</key><string>Owner</string>
			<key>LSItemContentTypes</key><array><string>dev.scrawl.document</string></array>
		</dict>
		<dict>
			<key>CFBundleTypeName</key><string>SVG Image</string>
			<key>CFBundleTypeRole</key><string>Editor</string>
			<key>LSHandlerRank</key><string>Alternate</string>
			<key>LSItemContentTypes</key><array><string>public.svg-image</string></array>
		</dict>
		<dict>
			<key>CFBundleTypeName</key><string>Design File</string>
			<key>CFBundleTypeRole</key><string>Editor</string>
			<key>LSHandlerRank</key><string>Alternate</string>
			<key>CFBundleTypeExtensions</key><array><string>sketch</string></array>
		</dict>
	</array>
	<key>UTExportedTypeDeclarations</key>
	<array>
		<dict>
			<key>UTTypeIdentifier</key><string>dev.scrawl.document</string>
			<key>UTTypeDescription</key><string>Scrawl Document</string>
			<key>UTTypeConformsTo</key><array><string>public.data</string><string>public.content</string></array>
			<key>UTTypeTagSpecification</key>
			<dict>
				<key>public.filename-extension</key><array><string>scrawl</string></array>
				<key>public.mime-type</key><string>application/x-scrawl</string>
			</dict>
		</dict>
	</array>
</dict>
</plist>
PLIST

plutil -lint "$app/Contents/Info.plist" >/dev/null

# Quick Look extensions: quicklook_extension NAME POINT CLASS FRAMEWORK SOURCE [ATTRIBUTES]
quicklook_extension() {
	appex="$app/Contents/PlugIns/Scrawl$1.appex"
	mkdir -p "$appex/Contents/MacOS"
	bins=
	for arch in $archs; do
		swiftc -O -parse-as-library -application-extension -module-name "Scrawl$1" \
			-target "$arch-apple-macos12.0" -framework "$4" \
			-Xlinker -e -Xlinker _NSExtensionMain \
			macos/quicklook/ScrawlPreview.swift "macos/quicklook/$5" -o "target/release/Scrawl$1-$arch"
		bins="$bins target/release/Scrawl$1-$arch"
	done
	lipo -create $bins -output "$appex/Contents/MacOS/Scrawl$1"
	cat > "$appex/Contents/Info.plist" <<EXT
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>Scrawl$1</string>
	<key>CFBundleDisplayName</key><string>Scrawl $1</string>
	<key>CFBundleIdentifier</key><string>dev.scrawl.Scrawl.$1</string>
	<key>CFBundleExecutable</key><string>Scrawl$1</string>
	<key>CFBundlePackageType</key><string>XPC!</string>
	<key>CFBundleShortVersionString</key><string>$version</string>
	<key>CFBundleVersion</key><string>$version</string>
	<key>LSMinimumSystemVersion</key><string>12.0</string>
	<key>NSExtension</key>
	<dict>
		<key>NSExtensionPointIdentifier</key><string>$2</string>
		<key>NSExtensionPrincipalClass</key><string>$3</string>
		<key>NSExtensionAttributes</key>
		<dict>
			<key>QLSupportedContentTypes</key><array><string>dev.scrawl.document</string></array>
			${6:-}
		</dict>
	</dict>
</dict>
</plist>
EXT
	plutil -lint "$appex/Contents/Info.plist" >/dev/null
	sign --entitlements macos/quicklook/extension.entitlements "$appex"
}
quicklook_extension Thumbnail com.apple.quicklook.thumbnail ThumbnailProvider QuickLookThumbnailing ThumbnailProvider.swift \
	'<key>QLThumbnailMinimumDimension</key><integer>0</integer>'
quicklook_extension Preview com.apple.quicklook.preview PreviewProvider QuickLookUI PreviewProvider.swift \
	'<key>QLIsDataBasedPreview</key><true/>'

# Signed (ad hoc at least), so macOS will launch it.
sign "$app"
# Tell Launch Services about the document types.
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app" 2>/dev/null || true
# And Quick Look about the extensions.
pluginkit -a "$app/Contents/PlugIns/"*.appex 2>/dev/null || true
echo "Built $app"
