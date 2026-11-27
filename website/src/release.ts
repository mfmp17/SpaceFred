// The release the site offers. The page and its JSON-LD (vite.config.ts) read from here; public/llms.txt
// and public/llms-full.txt repeat the version and download by hand.
export const RELEASE_VERSION = "0.3.7";
export const MIN_MACOS_LABEL = "macOS 14 or later";
export const SITE_URL = "https://anoma.ly/spaceman/";
export const DOWNLOAD_URL = `${SITE_URL}download/macos-arm64`;
export const CHECKSUM_URL = `${DOWNLOAD_URL}.sha256`;
export const INSTALL = `curl -fL ${DOWNLOAD_URL} \\
  -o /tmp/Spaceman.zip && ditto -x -k /tmp/Spaceman.zip /Applications \\
  && open /Applications/Spaceman.app`;
