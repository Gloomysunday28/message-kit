cask "lingdongdao" do
  version "0.1.0"
  sha256 "2e98cbc61072790314e87b3553e7c8ddba09724807ff97875c5ca071089b68a4"

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
