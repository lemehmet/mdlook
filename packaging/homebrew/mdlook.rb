# The formula served from lemehmet/homebrew-mdlook. This file is the template:
# the release workflow substitutes the underscore-fenced placeholders and
# pushes the result
# to the tap, so a change to the formula is made here, never in the tap repo.
#
# It installs the prebuilt release binaries rather than compiling — a personal
# tap has no bottle infrastructure, and without this every `brew install`
# would build syntect from source. The checksums pin each tarball to the exact
# bytes the release workflow tested.
class Mdlook < Formula
  desc "Terminal markdown reader that reflows to your terminal width"
  homepage "https://github.com/lemehmet/mdlook"
  version "__VERSION__"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/lemehmet/mdlook/releases/download/v__VERSION__/mdlook-v__VERSION__-aarch64-apple-darwin.tar.gz"
      sha256 "__SHA_AARCH64_APPLE__"
    end
    on_intel do
      url "https://github.com/lemehmet/mdlook/releases/download/v__VERSION__/mdlook-v__VERSION__-x86_64-apple-darwin.tar.gz"
      sha256 "__SHA_X86_64_APPLE__"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/lemehmet/mdlook/releases/download/v__VERSION__/mdlook-v__VERSION__-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "__SHA_X86_64_LINUX__"
    end
    on_arm do
      url "https://github.com/lemehmet/mdlook/releases/download/v__VERSION__/mdlook-v__VERSION__-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "__SHA_AARCH64_LINUX__"
    end
  end

  def install
    bin.install "mdlook"
    doc.install "README.md", "CHANGELOG.md"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mdlook --version")
  end
end
