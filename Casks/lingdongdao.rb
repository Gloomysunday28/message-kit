cask "lingdongdao" do
  version "0.1.1"
  sha256 "9ff769af450d57277e3f64c225dcc8bba97f7770dc8d0e8ea60d28ee3f90af3e"

  url "https://github.com/Gloomysunday28/message-kit/releases/download/v#{version}/LingDongDao_#{version}_aarch64.dmg"
  name "LingDongDao"
  desc "A Dynamic Island for the currently focused macOS app"
  homepage "https://github.com/Gloomysunday28/message-kit"

  depends_on arch: :arm64
  app "LingDongDao.app"

  zap trash: [
    "~/Library/Application Support/com.weiguang.lingdongdao",
    "~/Library/Preferences/com.weiguang.lingdongdao.plist",
  ]
end
