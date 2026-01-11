# frozen_string_literal: true

class Rehoboam < Formula
  desc "Real-time TUI for monitoring Claude Code agents"
  homepage "https://github.com/m-mohamed/rehoboam"
  url "https://github.com/m-mohamed/rehoboam/archive/refs/tags/v1.0.0.tar.gz"
  sha256 "PLACEHOLDER_SHA256"
  license "MIT"
  head "https://github.com/m-mohamed/rehoboam.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/rehoboam --version")
  end
end
