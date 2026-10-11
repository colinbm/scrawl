#!/bin/sh
# Builds target/release/Scrawl.app: the release binary in an app bundle that declares Scrawl's
# document type, so Finder opens .scrawl files with it (double-click, Open With, the Dock icon).
# Copy the app to /Applications to make it the default for .scrawl files.
set -eu
cd "$(dirname "$0")/.."

cargo build -p scrawl-app --release
version=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/scrawl-app/Cargo.toml | head -1)
app=target/release/Scrawl.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/scrawl "$app/Contents/MacOS/scrawl"

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
	<key>CFBundleDocumentTypes</key>
	<array>
		<dict>
			<key>CFBundleTypeName</key><string>Scrawl Document</string>
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
# Ad-hoc signing, so macOS will launch it.
codesign --force --sign - "$app" 2>/dev/null || true
# Tell Launch Services about the document types.
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app" 2>/dev/null || true
echo "Built $app"
