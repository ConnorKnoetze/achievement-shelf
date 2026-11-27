; Tauri's NSIS installer template, copied from tauri-cli v2.12.4
; (crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi) and set as
; bundle.windows.nsis.template in tauri.conf.json. Changes are marked "Achievement Shelf".
; When upgrading the Tauri CLI, copy the new upstream template and re-apply them:
; - The wizard is restyled in the shelf's look (theme block, "Shelf wizard" helpers): dark
;   window chrome, Bahnschrift headings, and the art in installer-art.bmp, which
;   bundle.windows.nsis.sidebarImage passes in as SIDEBARIMAGE.
; - The stock welcome, directory, finish and uninstall-confirm pages are replaced by custom
;   pages. A new Options page holds the desktop shortcut, Start menu shortcut and start with
;   Windows toggles, which the Install section applies. The uninstaller gains a Done page.
; - The progress pages close on their own so the wizard moves on, and show their log only
;   when something fails.
; Silent and passive runs, which the in-app updater uses, skip every page as before.
Unicode true
ManifestDPIAware true
; Add in `dpiAwareness` `PerMonitorV2` to manifest for Windows 10 1607+ (note this should not affect lower versions since they should be able to ignore this and pick up `dpiAware` `true` set by `ManifestDPIAware true`)
; Currently undocumented on NSIS's website but is in the Docs folder of source tree, see
; https://github.com/kichik/nsis/blob/5fc0b87b819a9eec006df4967d08e522ddd651c9/Docs/src/attributes.but#L286-L300
; https://github.com/tauri-apps/tauri/pull/10106
ManifestDPIAwareness PerMonitorV2

!if "{{compression}}" == "none"
  SetCompress off
!else
  ; Set the compression algorithm. We default to LZMA.
  SetCompressor /SOLID "{{compression}}"
!endif

; Keep above !include to stay ahead of any plugin command
; see https://github.com/tauri-apps/tauri/pull/15422#discussion_r3289239624
{{#if signed_plugins_path}}
!addplugindir "{{signed_plugins_path}}"
{{/if}}

!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
!include "utils.nsh"
!include "FileAssociation.nsh"
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "Win\RestartManager.nsh"
!include "StrFunc.nsh"
; Achievement Shelf: the custom pages.
!include nsDialogs.nsh
${StrCase}
${StrLoc}

{{#if installer_hooks}}
!include "{{installer_hooks}}"
{{/if}}

!define WEBVIEW2APPGUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

!define MANUFACTURER "{{manufacturer}}"
!define PRODUCTNAME "{{product_name}}"
!define VERSION "{{version}}"
!define VERSIONWITHBUILD "{{version_with_build}}"
!define HOMEPAGE "{{homepage}}"
!define INSTALLMODE "{{install_mode}}"
!define LICENSE "{{license}}"
!define INSTALLERICON "{{installer_icon}}"
!define SIDEBARIMAGE "{{sidebar_image}}"
!define HEADERIMAGE "{{header_image}}"
!define UNINSTALLERICON "{{uninstaller_icon}}"
!define UNINSTALLERHEADERIMAGE "{{uninstaller_header_image}}"
!define MAINBINARYNAME "{{main_binary_name}}"
!define MAINBINARYSRCPATH "{{main_binary_path}}"
!define BUNDLEID "{{bundle_id}}"
!define COPYRIGHT "{{copyright}}"
!define OUTFILE "{{out_file}}"
!define ARCH "{{arch}}"
!define ADDITIONALPLUGINSPATH "{{additional_plugins_path}}"
!define ALLOWDOWNGRADES "{{allow_downgrades}}"
!define DISPLAYLANGUAGESELECTOR "{{display_language_selector}}"
!define INSTALLWEBVIEW2MODE "{{install_webview2_mode}}"
!define WEBVIEW2INSTALLERARGS "{{webview2_installer_args}}"
!define WEBVIEW2BOOTSTRAPPERPATH "{{webview2_bootstrapper_path}}"
!define WEBVIEW2INSTALLERPATH "{{webview2_installer_path}}"
!define MINIMUMWEBVIEW2VERSION "{{minimum_webview2_version}}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define MANUKEY "Software\${MANUFACTURER}"
!define MANUPRODUCTKEY "${MANUKEY}\${PRODUCTNAME}"
!define UNINSTALLERSIGNCOMMAND "{{uninstaller_sign_cmd}}"
!define ESTIMATEDSIZE "{{estimated_size}}"
!define STARTMENUFOLDER "{{start_menu_folder}}"

Var PassiveMode
Var UpdateMode
Var NoShortcutMode
Var WixMode
Var OldMainBinaryName

; Achievement Shelf: the wizard's look, from src/window.css. SetCtlColors takes RRGGBB, and
; DWM takes a COLORREF, which is 0x00BBGGRR.
!define SHELF_GROUND "131518"
!define SHELF_PANEL "1A1D21"
!define SHELF_LINE "2A2F35"
!define SHELF_TEXT "EBE8E1"
!define SHELF_MUTED "9BA2AA"
!define SHELF_BRASS "E2C07A"
!define SHELF_GROUND_COLORREF 0x00181513
!define SHELF_LINE_COLORREF 0x00352F2A
!define SHELF_TEXT_COLORREF 0x00E1E8EB
; installer-art.bmp's size, so the art keeps its proportions at any display scale.
!define SHELF_ART_WIDTH 328
!define SHELF_ART_HEIGHT 628
!define MUI_BGCOLOR "${SHELF_GROUND}"
!define MUI_TEXTCOLOR "${SHELF_TEXT}"
!define SHELF_BRASS_COLORREF 0x007AC0E2
!define SHELF_PANEL_COLORREF 0x00211D1A
; The progress pages' log, shown only when something fails (see SHELF_SHOW_LOG).
!define MUI_INSTFILESPAGE_COLORS "${SHELF_TEXT} ${SHELF_PANEL}"
!define MUI_CUSTOMFUNCTION_GUIINIT ShelfGuiInit
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.ShelfGuiInit

Var ShelfPage
Var ShelfCtl
Var ShelfTextX
Var ShelfArtImage
Var ShelfFontKicker
Var ShelfFontTitle
Var ShelfFontStrong
Var ShelfFontBody
Var DesktopShortcutState
Var StartMenuShortcutState
Var AutostartState
Var DesktopShortcutToggle
Var StartMenuShortcutToggle
Var AutostartToggle
Var LaunchToggle
Var DirectoryField
Var DirectorySpace

; Achievement Shelf: Shelf wizard helpers, shared by the installer and uninstaller pages.
; They use $0-$2 and $9, so don't pass those registers in.

; A new nsDialogs page on the shelf's ground.
!macro SHELF_PAGE
  nsDialogs::Create 1018
  Pop $ShelfPage
  ${If} $ShelfPage == error
    Abort
  ${EndIf}
  ${IfThen} $(^RTL) = 1 ${|} nsDialogs::SetRTL $(^RTL) ${|}
  SetCtlColors $ShelfPage ${SHELF_TEXT} ${SHELF_GROUND}
!macroend

; An nsDialogs label. X, Y, W and H are as nsDialogs takes them.
!macro SHELF_LABEL X Y W H TEXT FONT COLOR
  ${NSD_CreateLabel} ${X} ${Y} ${W} ${H} "${TEXT}"
  Pop $ShelfCtl
  SendMessage $ShelfCtl ${WM_SETFONT} ${FONT} 1
  SetCtlColors $ShelfCtl ${COLOR} ${SHELF_GROUND}
!macroend

; A page heading: a brass kicker, the title in the condensed face and a muted subtitle of up
; to SUBH dialog units. X is in pixels and runs to the right edge; Y is in dialog units.
!macro SHELF_HEADING X Y SUBH KICKER TITLE SUBTITLE
  !define /math _SHELF_TITLE_Y ${Y} + 11
  !define /math _SHELF_SUB_Y ${Y} + 35
  ; nsDialogs reads a width of -0 as 0, not as the full width.
  !if "${X}" == "0"
    !define _SHELF_W 100%
  !else
    !define _SHELF_W -${X}
  !endif
  !insertmacro SHELF_LABEL ${X} ${Y}u ${_SHELF_W} 10u "${KICKER}" $ShelfFontKicker ${SHELF_BRASS}
  !insertmacro SHELF_LABEL ${X} ${_SHELF_TITLE_Y}u ${_SHELF_W} 22u "${TITLE}" $ShelfFontTitle ${SHELF_TEXT}
  !insertmacro SHELF_LABEL ${X} ${_SHELF_SUB_Y}u ${_SHELF_W} ${SUBH}u "${SUBTITLE}" $ShelfFontBody ${SHELF_MUTED}
  !undef _SHELF_TITLE_Y
  !undef _SHELF_SUB_Y
  !undef _SHELF_W
!macroend

; A full-width hairline at Y dialog units.
!macro SHELF_RULE Y
  ${NSD_CreateLabel} 0 ${Y}u 100% 1 ""
  Pop $ShelfCtl
  SetCtlColors $ShelfCtl ${SHELF_LINE} ${SHELF_LINE}
!macroend

; The art down the page's left edge at its own proportions. Sets $ShelfTextX, in pixels, to
; where text beside it starts.
!macro SHELF_ART
  !if "${SIDEBARIMAGE}" != ""
    ${NSD_CreateBitmap} 0 0 10u 100% ""
    Pop $ShelfCtl
    System::Call '*(i, i, i, i)p.r9'
    System::Call 'user32::GetClientRect(p $ShelfCtl, p r9)'
    System::Call '*$9(i, i, i, i.r1)'
    System::Free $9
    IntOp $0 $1 * ${SHELF_ART_WIDTH}
    IntOp $0 $0 / ${SHELF_ART_HEIGHT}
    ; SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
    System::Call 'user32::SetWindowPos(p $ShelfCtl, p 0, i 0, i 0, i r0, i r1, i 0x16)'
    ${If} $ShelfArtImage != ""
      ${NSD_FreeImage} $ShelfArtImage
    ${EndIf}
    ${NSD_SetStretchedImage} $ShelfCtl "$PLUGINSDIR\shelf-art.bmp" $ShelfArtImage
    IntOp $1 $0 / 7
    IntOp $ShelfTextX $0 + $1
  !else
    StrCpy $ShelfTextX 0
  !endif
!macroend

; Turns CTL, a 10u check box or radio button with no text, into a toggle: a dark glyph with
; a label beside it and an optional hint below. Themed buttons ignore SetCtlColors for their
; own text, which would stay black on the dark ground, so the label is a separate control
; that clicks the button. X is in pixels and Y in dialog units, as CTL was created with.
!macro SHELF_TOGGLE_LABEL UN CTL X Y TEXT HINT
  System::Call 'uxtheme::SetWindowTheme(p ${CTL}, w "DarkMode_Explorer", p 0)'
  SetCtlColors ${CTL} ${SHELF_TEXT} ${SHELF_GROUND}
  System::Call '*(i, i, i, i)p.r9'
  System::Call 'user32::GetClientRect(p ${CTL}, p r9)'
  System::Call '*$9(i, i, i.r0, i)'
  System::Free $9
  IntOp $1 $0 / 3
  IntOp $0 $0 + $1
  IntOp $0 $0 + ${X}
  !insertmacro SHELF_LABEL $0 ${Y}u -$0 10u "${TEXT}" $ShelfFontStrong ${SHELF_TEXT}
  nsDialogs::SetUserData $ShelfCtl ${CTL}
  ${NSD_OnClick} $ShelfCtl ${UN}ShelfToggleClick
  !if "${HINT}" != ""
    !define /math _SHELF_HINT_Y ${Y} + 11
    !insertmacro SHELF_LABEL $0 ${_SHELF_HINT_Y}u -$0 20u "${HINT}" $ShelfFontBody ${SHELF_MUTED}
    !undef _SHELF_HINT_Y
  !endif
!macroend

; A check box toggle in VAR, checked when STATE is 1.
!macro SHELF_TOGGLE UN VAR STATE X Y TEXT HINT
  ${NSD_CreateCheckbox} ${X} ${Y}u 10u 10u ""
  Pop ${VAR}
  !insertmacro SHELF_TOGGLE_LABEL "${UN}" ${VAR} ${X} ${Y} "${TEXT}" "${HINT}"
  ${If} ${STATE} = 1
    ${NSD_Check} ${VAR}
  ${EndIf}
!macroend

!macro SHELF_BUTTON_TEXT ID TEXT
  GetDlgItem $0 $HWNDPARENT ${ID}
  SendMessage $0 ${WM_SETTEXT} 0 "STR:${TEXT}"
!macroend

; The last page of a wizard: one button, which closes it, in the far-right slot.
!macro SHELF_LAST_PAGE TEXT
  !insertmacro SHELF_BUTTON_TEXT 1 "${TEXT}"
  GetDlgItem $0 $HWNDPARENT 3
  ShowWindow $0 ${SW_HIDE}
  GetDlgItem $0 $HWNDPARENT 2
  ShowWindow $0 ${SW_HIDE}
  System::Call '*(i, i, i, i)p.r9'
  System::Call 'user32::GetWindowRect(p r0, p r9)'
  System::Call 'user32::MapWindowPoints(p 0, p $HWNDPARENT, p r9, i 1)'
  System::Call '*$9(i.r1, i.r2, i, i)'
  System::Free $9
  GetDlgItem $0 $HWNDPARENT 1
  ; SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE
  System::Call 'user32::SetWindowPos(p r0, p 0, i r1, i r2, i 0, i 0, i 0x15)'
!macroend

; Moves CTL, a child of PARENT, to Y and H dialog units (PARENT's) across PARENT's full width.
!macro SHELF_PLACE PARENT CTL Y H
  System::Call '*(i 0, i ${Y}, i 0, i ${H})p.r9'
  System::Call 'user32::MapDialogRect(p ${PARENT}, p r9)'
  System::Call '*$9(i, i.r1, i, i.r2)'
  System::Free $9
  System::Call '*(i, i, i, i)p.r9'
  System::Call 'user32::GetClientRect(p ${PARENT}, p r9)'
  System::Call '*$9(i, i, i.r0, i)'
  System::Free $9
  ; SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED
  System::Call 'user32::SetWindowPos(p ${CTL}, p 0, i 0, i r1, i r0, i r2, i 0x34)'
!macroend

; A label on a progress page, which is a stock NSIS dialog rather than an nsDialogs page.
; Backticks, so TEXT can hold an apostrophe.
!macro SHELF_INST_LABEL Y H TEXT FONT COLOR
  System::Call `user32::CreateWindowEx(i 0, w "STATIC", w "${TEXT}", i 0x50000000, i 0, i 0, i 0, i 0, p $ShelfPage, p 0, p 0, p 0) p.s`
  Pop $ShelfCtl
  SendMessage $ShelfCtl ${WM_SETFONT} ${FONT} 1
  SetCtlColors $ShelfCtl ${COLOR} ${SHELF_GROUND}
  !insertmacro SHELF_PLACE $ShelfPage $ShelfCtl ${Y} ${H}
!macroend

; Restyles a progress page: heading, a muted status line and a thin brass bar. The log and
; its "Show details" button stay hidden unless something fails (see SHELF_SHOW_LOG).
!macro SHELF_INSTFILES_SHOW KICKER TITLE SUBTITLE
  FindWindow $ShelfPage "#32770" "" $HWNDPARENT
  SetCtlColors $ShelfPage ${SHELF_TEXT} ${SHELF_GROUND}
  !insertmacro SHELF_INST_LABEL 0 10 "${KICKER}" $ShelfFontKicker ${SHELF_BRASS}
  !insertmacro SHELF_INST_LABEL 11 22 "${TITLE}" $ShelfFontTitle ${SHELF_TEXT}
  !insertmacro SHELF_INST_LABEL 35 20 "${SUBTITLE}" $ShelfFontBody ${SHELF_MUTED}
  GetDlgItem $ShelfCtl $ShelfPage 1006
  SendMessage $ShelfCtl ${WM_SETFONT} $ShelfFontBody 1
  SetCtlColors $ShelfCtl ${SHELF_MUTED} ${SHELF_GROUND}
  !insertmacro SHELF_PLACE $ShelfPage $ShelfCtl 64 12
  ; The progress bar as a flat slot: brass filling the panel colour. A themed bar ignores
  ; colours, so drop the theme, then the classic border (WS_BORDER and the client and static
  ; edges) that an unthemed bar draws.
  GetDlgItem $ShelfCtl $ShelfPage 1004
  System::Call 'uxtheme::SetWindowTheme(p $ShelfCtl, w " ", w " ")'
  ; PBM_SETBARCOLOR, PBM_SETBKCOLOR
  SendMessage $ShelfCtl 0x0409 0 ${SHELF_BRASS_COLORREF}
  SendMessage $ShelfCtl 0x2001 0 ${SHELF_PANEL_COLORREF}
  System::Call 'user32::GetWindowLong(p $ShelfCtl, i -16) i .r0'
  IntOp $0 $0 & 0xFF7FFFFF
  System::Call 'user32::SetWindowLong(p $ShelfCtl, i -16, i r0)'
  System::Call 'user32::GetWindowLong(p $ShelfCtl, i -20) i .r0'
  IntOp $0 $0 & 0xFFFDFDFF
  System::Call 'user32::SetWindowLong(p $ShelfCtl, i -20, i r0)'
  !insertmacro SHELF_PLACE $ShelfPage $ShelfCtl 80 4
  GetDlgItem $ShelfCtl $ShelfPage 1016
  ShowWindow $ShelfCtl ${SW_HIDE}
  GetDlgItem $ShelfCtl $ShelfPage 1027
  ShowWindow $ShelfCtl ${SW_HIDE}
!macroend

; Brings back a progress page's log, so a failed install or uninstall shows why.
!macro SHELF_SHOW_LOG
  ${IfNot} ${Silent}
    FindWindow $ShelfPage "#32770" "" $HWNDPARENT
    GetDlgItem $ShelfCtl $ShelfPage 1016
    !insertmacro SHELF_PLACE $ShelfPage $ShelfCtl 94 60
    ShowWindow $ShelfCtl ${SW_SHOW}
  ${EndIf}
!macroend

!macro SHELF_HIDE_ITEM ID
  GetDlgItem $0 $HWNDPARENT ${ID}
  ShowWindow $0 ${SW_HIDE}
!macroend

!macro SHELF_DARK_BUTTON ID
  GetDlgItem $0 $HWNDPARENT ${ID}
  System::Call 'uxtheme::SetWindowTheme(p r0, w "DarkMode_Explorer", p 0)'
  SetCtlColors $0 ${SHELF_TEXT} ${SHELF_GROUND}
!macroend

; Functions both the installer and the uninstaller need, once with each prefix.
!macro SHELF_FUNCTIONS UN
  ; Runs once the window exists, before the first page.
  Function ${UN}ShelfGuiInit
    ; A dark title bar. Windows 11 also takes the caption, border and caption text colours;
    ; older versions ignore those attributes.
    System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 20, *i 1, i 4)'
    System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 35, *i ${SHELF_GROUND_COLORREF}, i 4)'
    System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 34, *i ${SHELF_LINE_COLORREF}, i 4)'
    System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 36, *i ${SHELF_TEXT_COLORREF}, i 4)'
    SetCtlColors $HWNDPARENT ${SHELF_TEXT} ${SHELF_GROUND}

    ; Each page draws its own heading, so hide MUI's header strip and image, the lines and
    ; the branding text.
    !insertmacro SHELF_HIDE_ITEM 1034
    !insertmacro SHELF_HIDE_ITEM 1037
    !insertmacro SHELF_HIDE_ITEM 1038
    !insertmacro SHELF_HIDE_ITEM 1039
    !insertmacro SHELF_HIDE_ITEM 1046
    !insertmacro SHELF_HIDE_ITEM 1035
    !insertmacro SHELF_HIDE_ITEM 1036
    !insertmacro SHELF_HIDE_ITEM 1045
    !insertmacro SHELF_HIDE_ITEM 1028
    !insertmacro SHELF_HIDE_ITEM 1256

    !insertmacro SHELF_DARK_BUTTON 1
    !insertmacro SHELF_DARK_BUTTON 2
    !insertmacro SHELF_DARK_BUTTON 3

    ; Grow the page area up into the space the header used, leaving a margin and a half.
    GetDlgItem $ShelfCtl $HWNDPARENT 1018
    System::Call '*(i, i, i, i)p.r9'
    System::Call 'user32::GetWindowRect(p $ShelfCtl, p r9)'
    System::Call 'user32::MapWindowPoints(p 0, p $HWNDPARENT, p r9, i 2)'
    System::Call '*$9(i.r0, i.r1, i.r2, i.r3)'
    System::Free $9
    IntOp $1 $0 * 3
    IntOp $1 $1 / 2
    IntOp $2 $2 - $0
    IntOp $3 $3 - $1
    ; SWP_NOZORDER | SWP_NOACTIVATE
    System::Call 'user32::SetWindowPos(p $ShelfCtl, p 0, i r0, i r1, i r2, i r3, i 0x14)'

    ; The shelf's grade labels are Barlow Condensed, which ships only as woff2. Bahnschrift
    ; SemiBold Condensed (Windows 10 1709 and later) is the closest face Windows has.
    ${If} ${FileExists} "$FONTS\bahnschrift.ttf"
      CreateFont $ShelfFontKicker "Bahnschrift SemiBold Condensed" 10 400
      CreateFont $ShelfFontTitle "Bahnschrift SemiBold Condensed" 22 400
    ${Else}
      CreateFont $ShelfFontKicker "Segoe UI Semibold" 8 400
      CreateFont $ShelfFontTitle "Segoe UI Semibold" 18 400
    ${EndIf}
    CreateFont $ShelfFontStrong "Segoe UI Semibold" 9 400
    CreateFont $ShelfFontBody "Segoe UI" 9 400

    !if "${SIDEBARIMAGE}" != ""
      InitPluginsDir
      File "/oname=$PLUGINSDIR\shelf-art.bmp" "${SIDEBARIMAGE}"
    !endif
  FunctionEnd

  ; A toggle's label was clicked: click its button (see SHELF_TOGGLE_LABEL), unless the
  ; button is disabled.
  Function ${UN}ShelfToggleClick
    Pop $0
    nsDialogs::GetUserData $0
    Pop $0
    System::Call 'user32::IsWindowEnabled(p r0) i .r1'
    ${If} $1 <> 0
      SendMessage $0 ${BM_CLICK} 0 0
    ${EndIf}
  FunctionEnd
!macroend
!insertmacro SHELF_FUNCTIONS ""
!insertmacro SHELF_FUNCTIONS "un."

Name "${PRODUCTNAME}"
BrandingText "${COPYRIGHT}"
OutFile "${OUTFILE}"

; We don't actually use this value as default install path,
; it's just for nsis to append the product name folder in the directory selector
; https://nsis.sourceforge.io/Reference/InstallDir
!define PLACEHOLDER_INSTALL_DIR "placeholder\${PRODUCTNAME}"
InstallDir "${PLACEHOLDER_INSTALL_DIR}"

VIProductVersion "${VERSIONWITHBUILD}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "LegalCopyright" "${COPYRIGHT}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

# additional plugins
!addplugindir "${ADDITIONALPLUGINSPATH}"

; Uninstaller signing command
!if "${UNINSTALLERSIGNCOMMAND}" != ""
  !uninstfinalize '${UNINSTALLERSIGNCOMMAND}'
!endif

; Handle install mode, `perUser`, `perMachine` or `both`
!if "${INSTALLMODE}" == "perMachine"
  RequestExecutionLevel admin
!endif

!if "${INSTALLMODE}" == "currentUser"
  RequestExecutionLevel user
!endif

!if "${INSTALLMODE}" == "both"
  !define MULTIUSER_MUI
  !define MULTIUSER_INSTALLMODE_INSTDIR "${PRODUCTNAME}"
  !define MULTIUSER_INSTALLMODE_COMMANDLINE
  !if "${ARCH}" == "x64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !else if "${ARCH}" == "arm64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !endif
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_KEY "${UNINSTKEY}"
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_VALUENAME "CurrentUser"
  !define MULTIUSER_INSTALLMODEPAGE_SHOWUSERNAME
  !define MULTIUSER_INSTALLMODE_FUNCTION RestorePreviousInstallLocation
  !define MULTIUSER_EXECUTIONLEVEL Highest
  !include MultiUser.nsh
!endif

; Installer icon
!if "${INSTALLERICON}" != ""
  !define MUI_ICON "${INSTALLERICON}"
!endif

; Installer sidebar image
!if "${SIDEBARIMAGE}" != ""
  !define MUI_WELCOMEFINISHPAGE_BITMAP "${SIDEBARIMAGE}"
!endif

; Enable header images for installer and uninstaller pages when either image is configured.
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!else if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!endif

; Installer header image
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_BITMAP "${HEADERIMAGE}"
!endif

; Uninstaller header image
!if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_UNBITMAP "${UNINSTALLERHEADERIMAGE}"
!endif

; Uninstaller icon
!if "${UNINSTALLERICON}" != ""
  !define MUI_UNICON "${UNINSTALLERICON}"
!endif

; Define registry key to store installer language
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "${MANUPRODUCTKEY}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Installer Language"

; Installer pages, must be ordered as they appear
; 1. Welcome Page
; Achievement Shelf: a custom page in place of MUI_PAGE_WELCOME.
Page custom PageWelcome
Function PageWelcome
  Call SkipIfPassive
  !insertmacro SHELF_PAGE
  !insertmacro SHELF_ART
  !insertmacro SHELF_HEADING $ShelfTextX 30 30 "SETUP" "${PRODUCTNAME}" "Your Steam and GOG Galaxy achievements on a set of shelves in the corner of your screen."
  !insertmacro SHELF_LABEL $ShelfTextX 98u -$ShelfTextX 10u "Version ${VERSION}" $ShelfFontStrong ${SHELF_BRASS}
  !insertmacro SHELF_LABEL $ShelfTextX 116u -$ShelfTextX 30u "Click Next to choose where to install it." $ShelfFontBody ${SHELF_TEXT}
  nsDialogs::Show
FunctionEnd

; 2. License Page (if defined)
!if "${LICENSE}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !insertmacro MUI_PAGE_LICENSE "${LICENSE}"
!endif

; 3. Install mode (if it is set to `both`)
!if "${INSTALLMODE}" == "both"
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !insertmacro MULTIUSER_PAGE_INSTALLMODE
!endif

; 4. Custom page to ask user if he wants to reinstall/uninstall
;    only if a previous installation was detected
Var ReinstallPageCheck
Page custom PageReinstall PageLeaveReinstall

; Find a previous WiX installation and set $R6 to its uninstall registry key, or to "" if none.
;
; A WiX installer stores the installation info in registry
; using a UUID and so we have to loop through all keys under
; `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
; and check if `DisplayName` and `Publisher` keys match ${PRODUCTNAME} and ${MANUFACTURER}
;
; This has a potential issue that there maybe another installation that matches
; our ${PRODUCTNAME} and ${MANUFACTURER} but wasn't installed by our WiX installer,
; however, this should be fine since the user will have to confirm the uninstallation
; and they can chose to abort it if doesn't make sense.
Function FindWixInstall
  StrCpy $R6 ""
  StrCpy $0 0
  wix_loop:
    EnumRegKey $1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $0
    StrCmp $1 "" wix_loop_done ; Exit loop if there is no more keys to loop on
    IntOp $0 $0 + 1
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "Publisher"
    StrCmp "$R0$R1" "${PRODUCTNAME}${MANUFACTURER}" 0 wix_loop
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "UninstallString"
    ${StrCase} $R1 $R0 "L"
    ${StrLoc} $R0 $R1 "msiexec" ">"
    StrCmp $R0 0 0 wix_loop_done
    StrCpy $R6 "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1"
  wix_loop_done:
FunctionEnd

Function PageReinstall
  ; Uninstall previous WiX installation if exists.
  Call FindWixInstall
  ${If} $R6 != ""
    StrCpy $WixMode 1
    Goto compare_version
  ${EndIf}

  ; Check if there is an existing installation, if not, abort the reinstall page
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" ""
  ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
  ${IfThen} "$R0$R1" == "" ${|} Abort ${|}

  ; Compare this installar version with the existing installation
  ; and modify the messages presented to the user accordingly
  compare_version:
  StrCpy $R4 "$(older)"
  ${If} $WixMode = 1
    ReadRegStr $R0 HKLM "$R6" "DisplayVersion"
  ${Else}
    ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  ${EndIf}
  ${IfThen} $R0 == "" ${|} StrCpy $R4 "$(unknown)" ${|}

  nsis_tauri_utils::SemverCompare "${VERSION}" $R0
  Pop $R0

  ; Read the install choice header through $R5 so a custom language file that only
  ; defines the misspelled `choowHowToInstall` still shows its own text
  ; TODO: remove the `choowHowToInstall` fallback in a future major version
  StrCpy $R5 "$(chooseHowToInstall)"
  ${IfThen} $R5 == "" ${|} StrCpy $R5 "$(choowHowToInstall)" ${|}

  ; Reinstalling the same version
  ${If} $R0 = 0
    StrCpy $R1 "$(alreadyInstalledLong)"
    StrCpy $R2 "$(addOrReinstall)"
    StrCpy $R3 "$(uninstallApp)"
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$(chooseMaintenanceOption)"
  ; Upgrading
  ${ElseIf} $R0 = 1
    StrCpy $R1 "$(olderOrUnknownVersionInstalled)"
    StrCpy $R2 "$(uninstallBeforeInstalling)"
    StrCpy $R3 "$(dontUninstall)"
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$R5"
  ; Downgrading
  ${ElseIf} $R0 = -1
    StrCpy $R1 "$(newerVersionInstalled)"
    StrCpy $R2 "$(uninstallBeforeInstalling)"
    !if "${ALLOWDOWNGRADES}" == "true"
      StrCpy $R3 "$(dontUninstall)"
    !else
      StrCpy $R3 "$(dontUninstallDowngrade)"
    !endif
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$R5"
  ${Else}
    Abort
  ${EndIf}

  ; Skip showing the page if passive
  ;
  ; Note that we don't call this earlier at the beginning
  ; of this function because we need to populate some variables
  ; related to current installed version if detected and whether
  ; we are downgrading or not.
  ${If} $PassiveMode = 1
    Call PageLeaveReinstall
  ${Else}
    ; Achievement Shelf: the page in the wizard's style. Each choice is a radio glyph with
    ; its text in a label beside it (see SHELF_TOGGLE_LABEL).
    !insertmacro SHELF_PAGE
    !insertmacro SHELF_HEADING 0 0 42 "BEFORE YOU INSTALL" "$(alreadyInstalled)" "$R1"
    !insertmacro SHELF_RULE 84

    StrCpy $R1 $R2
    ${NSD_CreateRadioButton} 0 96u 10u 10u ""
    Pop $R2
    !insertmacro SHELF_TOGGLE_LABEL "" $R2 0 96 "$R1" ""
    ${NSD_OnClick} $R2 PageReinstallUpdateSelection

    StrCpy $R1 $R3
    ${NSD_CreateRadioButton} 0 114u 10u 10u ""
    Pop $R3
    !insertmacro SHELF_TOGGLE_LABEL "" $R3 0 114 "$R1" ""
    ; Disable this radio button if downgrading and downgrades are disabled
    !if "${ALLOWDOWNGRADES}" == "false"
      ${If} $R0 = -1
        EnableWindow $R3 0
        ; Achievement Shelf: dim its label to match.
        SetCtlColors $ShelfCtl ${SHELF_MUTED} ${SHELF_GROUND}
      ${EndIf}
    !endif
    ${NSD_OnClick} $R3 PageReinstallUpdateSelection

    ; Check the first radio button if this the first time
    ; we enter this page or if the second button wasn't
    ; selected the last time we were on this page
    ${If} $ReinstallPageCheck <> 2
      SendMessage $R2 ${BM_SETCHECK} ${BST_CHECKED} 0
    ${Else}
      SendMessage $R3 ${BM_SETCHECK} ${BST_CHECKED} 0
    ${EndIf}

    ${NSD_SetFocus} $R2
    nsDialogs::Show
  ${EndIf}
FunctionEnd
Function PageReinstallUpdateSelection
  ${NSD_GetState} $R2 $R1
  ${If} $R1 == ${BST_CHECKED}
    StrCpy $ReinstallPageCheck 1
  ${Else}
    StrCpy $ReinstallPageCheck 2
  ${EndIf}
FunctionEnd
Function PageLeaveReinstall
  ${NSD_GetState} $R2 $R1

  ; If migrating from Wix, always uninstall
  ${If} $WixMode = 1
    Goto reinst_uninstall
  ${EndIf}

  ; In update mode, always proceeds without uninstalling
  ${If} $UpdateMode = 1
    Goto reinst_done
  ${EndIf}

  ; $R0 holds whether same(0)/upgrading(1)/downgrading(-1) version
  ; $R1 holds the radio buttons state:
  ;   1 => first choice was selected
  ;   0 => second choice was selected
  ${If} $R0 = 0 ; Same version, proceed
    ${If} $R1 = 1              ; User chose to add/reinstall
      Goto reinst_done
    ${Else}                    ; User chose to uninstall
      Goto reinst_uninstall
    ${EndIf}
  ${ElseIf} $R0 = 1 ; Upgrading
    ${If} $R1 = 1              ; User chose to uninstall
      Goto reinst_uninstall
    ${Else}
      Goto reinst_done         ; User chose NOT to uninstall
    ${EndIf}
  ${ElseIf} $R0 = -1 ; Downgrading
    ${If} $R1 = 1              ; User chose to uninstall
      Goto reinst_uninstall
    ${Else}
      Goto reinst_done         ; User chose NOT to uninstall
    ${EndIf}
  ${EndIf}

  reinst_uninstall:
    HideWindow
    ClearErrors

    ${If} $WixMode = 1
      ReadRegStr $R1 HKLM "$R6" "UninstallString"
      ExecWait '$R1' $0
    ${Else}
      ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
      ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
      ${IfThen} $UpdateMode = 1 ${|} StrCpy $R1 "$R1 /UPDATE" ${|} ; append /UPDATE
      ${IfThen} $PassiveMode = 1 ${|} StrCpy $R1 "$R1 /P" ${|} ; append /P
      StrCpy $R1 "$R1 _?=$4" ; append uninstall directory
      ExecWait '$R1' $0
    ${EndIf}

    BringToFront

    ${IfThen} ${Errors} ${|} StrCpy $0 2 ${|} ; ExecWait failed, set fake exit code

    ${If} $0 <> 0
    ${OrIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
      ; User cancelled wix uninstaller? return to select un/reinstall page
      ${If} $WixMode = 1
      ${AndIf} $0 = 1602
        Abort
      ${EndIf}

      ; User cancelled NSIS uninstaller? return to select un/reinstall page
      ${If} $0 = 1
        Abort
      ${EndIf}

      ; Other errors? show generic error message and return to select un/reinstall page
      MessageBox MB_ICONEXCLAMATION "$(unableToUninstall)"
      Abort
    ${EndIf}

    ; The NSIS uninstaller was run in place (`_?=`), so it cannot delete itself
    ; and the install directory, clean them up here once the uninstall succeeded
    ; https://nsis.sourceforge.io/Docs/Chapter3.html#:~:text=path%20contains%20spaces.-,3.2.3%20Examples,-installer.exe%20/NCRC
    ${If} $WixMode = 0
    ${AndIf} $4 != ""
      Delete "$4\uninstall.exe"
      RMDir "$4"
    ${EndIf}
  reinst_done:
FunctionEnd

; 5. Choose install directory page
; Achievement Shelf: a custom page in place of MUI_PAGE_DIRECTORY.
Page custom PageDirectory PageDirectoryLeave
Function PageDirectory
  Call SkipIfPassive
  !insertmacro SHELF_PAGE
  !insertmacro SHELF_HEADING 0 0 20 "STEP 1 OF 3" "Install location" "${PRODUCTNAME} will be installed in this folder. The suggested one suits most people."
  !insertmacro SHELF_RULE 62

  !insertmacro SHELF_LABEL 0 74u 100% 10u "Folder" $ShelfFontStrong ${SHELF_TEXT}
  ${NSD_CreateText} 0 87u -64u 14u "$INSTDIR"
  Pop $DirectoryField
  SendMessage $DirectoryField ${WM_SETFONT} $ShelfFontBody 1
  ; A flat field on the panel colour: no theme and no light sunken edge, with some padding.
  SetCtlColors $DirectoryField ${SHELF_TEXT} ${SHELF_PANEL}
  System::Call 'uxtheme::SetWindowTheme(p $DirectoryField, w " ", w " ")'
  System::Call 'user32::GetWindowLong(p $DirectoryField, i -20) i .r0'
  IntOp $0 $0 & 0xFFFFFDFF
  System::Call 'user32::SetWindowLong(p $DirectoryField, i -20, i r0)'
  ; SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED
  System::Call 'user32::SetWindowPos(p $DirectoryField, p 0, i 0, i 0, i 0, i 0, i 0x37)'
  ; EM_SETMARGINS, both sides
  SendMessage $DirectoryField 0x00D3 3 0x00060006
  ${NSD_OnChange} $DirectoryField PageDirectoryChanged

  ${NSD_CreateButton} -58u 86u 58u 16u "Browse..."
  Pop $ShelfCtl
  System::Call 'uxtheme::SetWindowTheme(p $ShelfCtl, w "DarkMode_Explorer", p 0)'
  SetCtlColors $ShelfCtl ${SHELF_TEXT} ${SHELF_GROUND}
  ${NSD_OnClick} $ShelfCtl PageDirectoryBrowse

  !insertmacro SHELF_LABEL 0 108u 100% 20u "" $ShelfFontBody ${SHELF_MUTED}
  StrCpy $DirectorySpace $ShelfCtl
  Call PageDirectoryUpdateSpace
  nsDialogs::Show
FunctionEnd

Function PageDirectoryChanged
  Pop $0
  Call PageDirectoryUpdateSpace
FunctionEnd

; Shows the space the app needs and what's free on the folder's drive.
Function PageDirectoryUpdateSpace
  ${NSD_GetText} $DirectoryField $0
  ; ESTIMATEDSIZE is in KB; round up to whole MB.
  IntOp $1 ${ESTIMATEDSIZE} + 1023
  IntOp $1 $1 / 1024
  ${GetRoot} $0 $2
  ${If} $2 == ""
    ${NSD_SetText} $DirectorySpace "Needs about $1 MB."
    Return
  ${EndIf}
  ClearErrors
  ${DriveSpace} "$2\" "/D=F /S=M" $3
  ${If} ${Errors}
    ${NSD_SetText} $DirectorySpace "Needs about $1 MB. Drive $2 isn't available."
    Return
  ${EndIf}
  ${If} $3 >= 1024
    IntOp $4 $3 / 1024
    IntOp $5 $3 % 1024
    IntOp $5 $5 * 10
    IntOp $5 $5 / 1024
    StrCpy $3 "$4.$5 GB"
  ${Else}
    StrCpy $3 "$3 MB"
  ${EndIf}
  ${NSD_SetText} $DirectorySpace "Needs about $1 MB, and $2 has $3 free."
FunctionEnd

Function PageDirectoryBrowse
  Pop $0
  ${NSD_GetText} $DirectoryField $1
  nsDialogs::SelectFolderDialog "Choose where to install ${PRODUCTNAME}" $1
  Pop $0
  ${If} $0 == error
    Return
  ${EndIf}
  ; Install into a folder of its own inside the one picked, as the stock page did.
  StrCpy $1 $0 1 -1
  ${If} $1 == "\"
    StrCpy $0 $0 -1
  ${EndIf}
  StrLen $1 "${PRODUCTNAME}"
  IntOp $1 0 - $1
  StrCpy $2 $0 "" $1
  ${If} $2 != "${PRODUCTNAME}"
    StrCpy $0 "$0\${PRODUCTNAME}"
  ${EndIf}
  ${NSD_SetText} $DirectoryField $0
FunctionEnd

; Checks the folder the stock page would have refused, then makes it $INSTDIR.
Function PageDirectoryLeave
  ${NSD_GetText} $DirectoryField $0
  StrCpy $1 $0 1 -1
  StrLen $2 $0
  ${If} $1 == "\"
  ${AndIf} $2 > 3
    StrCpy $0 $0 -1
  ${EndIf}

  ${GetRoot} $0 $1
  ${If} $1 == ""
    MessageBox MB_ICONEXCLAMATION|MB_OK "Enter a full folder path, such as C:\Apps\${PRODUCTNAME}."
    Abort
  ${EndIf}

  StrCpy $3 0
  ${Do}
    StrCpy $4 '*?"<>|' 1 $3
    ${If} $4 == ""
      ${ExitDo}
    ${EndIf}
    ${StrLoc} $5 $0 $4 ">"
    ${If} $5 != ""
      MessageBox MB_ICONEXCLAMATION|MB_OK 'A folder name can$\'t contain any of these characters: * ? " < > |'
      Abort
    ${EndIf}
    IntOp $3 $3 + 1
  ${Loop}

  ClearErrors
  ${DriveSpace} "$1\" "/D=F /S=M" $2
  ${If} ${Errors}
    MessageBox MB_ICONEXCLAMATION|MB_OK "Drive $1 isn't available. Choose another folder."
    Abort
  ${EndIf}
  IntOp $3 ${ESTIMATEDSIZE} + 1023
  IntOp $3 $3 / 1024
  ${If} $2 < $3
    MessageBox MB_ICONEXCLAMATION|MB_OK "There isn't enough free space on $1. Choose another drive."
    Abort
  ${EndIf}

  StrCpy $INSTDIR $0
FunctionEnd

; 6. Start menu shortcut page
Var AppStartMenuFolder
!if "${STARTMENUFOLDER}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !define MUI_STARTMENUPAGE_DEFAULTFOLDER "${STARTMENUFOLDER}"
!else
  !define MUI_PAGE_CUSTOMFUNCTION_PRE Skip
!endif
!insertmacro MUI_PAGE_STARTMENU Application $AppStartMenuFolder

; Achievement Shelf: 6b. Options page, for the shortcuts and starting with Windows. The
; Install section applies the choices; silent and passive installs keep the defaults .onInit
; sets.
Page custom PageOptions PageOptionsLeave
Function PageOptions
  Call SkipIfPassive
  !insertmacro SHELF_PAGE
  !insertmacro SHELF_HEADING 0 0 20 "STEP 2 OF 3" "Shortcuts and startup" "Choose how you'd like to open ${PRODUCTNAME}. Start with Windows can be changed later from the tray icon."
  !insertmacro SHELF_RULE 62
  !insertmacro SHELF_TOGGLE "" $DesktopShortcutToggle $DesktopShortcutState 0 74 "Desktop shortcut" "An icon on your desktop."
  !insertmacro SHELF_TOGGLE "" $StartMenuShortcutToggle $StartMenuShortcutState 0 100 "Start menu shortcut" "Find it by searching the Start menu."
  !insertmacro SHELF_TOGGLE "" $AutostartToggle $AutostartState 0 126 "Start with Windows" "Puts the shelf on your desktop each time you sign in."
  !insertmacro SHELF_BUTTON_TEXT 1 "Install"
  nsDialogs::Show
FunctionEnd
Function PageOptionsLeave
  ${NSD_GetState} $DesktopShortcutToggle $DesktopShortcutState
  ${NSD_GetState} $StartMenuShortcutToggle $StartMenuShortcutState
  ${NSD_GetState} $AutostartToggle $AutostartState
FunctionEnd

; 7. Installation page
; Achievement Shelf: restyled (see SHELF_INSTFILES_SHOW).
!define MUI_PAGE_CUSTOMFUNCTION_SHOW InstFilesShow
!insertmacro MUI_PAGE_INSTFILES
Function InstFilesShow
  !insertmacro SHELF_INSTFILES_SHOW "STEP 3 OF 3" "Installing" "Putting ${PRODUCTNAME} on your PC."
FunctionEnd

; 8. Finish page
; Achievement Shelf: a custom page in place of MUI_PAGE_FINISH. The desktop shortcut and start
; with Windows check boxes that MUI's page had are on the Options page now.
Page custom PageFinish PageFinishLeave
Function PageFinish
  Call SkipIfPassive
  !insertmacro SHELF_PAGE
  !insertmacro SHELF_ART
  !insertmacro SHELF_HEADING $ShelfTextX 30 40 "ALL SET" "Your shelf is ready" "${PRODUCTNAME} sits in the corner of your screen. Its tray icon has the rest: choosing trophies, settings and more."
  ${NSD_CreateCheckbox} $ShelfTextX 112u 10u 10u ""
  Pop $LaunchToggle
  !insertmacro SHELF_TOGGLE_LABEL "" $LaunchToggle $ShelfTextX 112 "Launch ${PRODUCTNAME} now" ""
  ${NSD_Check} $LaunchToggle
  !insertmacro SHELF_LAST_PAGE "Finish"
  nsDialogs::Show
FunctionEnd
Function PageFinishLeave
  ${NSD_GetState} $LaunchToggle $0
  ${If} $0 = ${BST_CHECKED}
    Call RunMainBinary
  ${EndIf}
FunctionEnd

Function RunMainBinary
  nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" ""
FunctionEnd

; Achievement Shelf: start with Windows.
; The Install section writes the same HKCU Run value as tauri-plugin-autostart (named after
; the product), so the tray's "Start with Windows" item shows the choice.
!define AUTOSTART_RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define AUTOSTART_APPROVEDKEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"

; Uninstaller Pages
; 1. Confirm uninstall page
; Achievement Shelf: a custom page in place of MUI_UNPAGE_CONFIRM and its added check box.
Var DeleteAppDataCheckbox
Var DeleteAppDataCheckboxState
UninstPage custom un.PageConfirm un.PageConfirmLeave
Function un.PageConfirm
  Call un.SkipIfPassive
  !insertmacro SHELF_PAGE
  !insertmacro SHELF_ART
  !insertmacro SHELF_HEADING $ShelfTextX 30 40 "UNINSTALL" "Remove the shelf?" "This removes ${PRODUCTNAME} and its shortcuts from $INSTDIR."
  ${NSD_CreateCheckbox} $ShelfTextX 112u 10u 10u ""
  Pop $DeleteAppDataCheckbox
  !insertmacro SHELF_TOGGLE_LABEL "un." $DeleteAppDataCheckbox $ShelfTextX 112 "Also delete my shelf data" "Your trophy picks, levels and settings. Leave this off to keep them for next time."
  !insertmacro SHELF_BUTTON_TEXT 1 "Uninstall"
  nsDialogs::Show
FunctionEnd
Function un.PageConfirmLeave
  ${NSD_GetState} $DeleteAppDataCheckbox $DeleteAppDataCheckboxState
FunctionEnd

; 2. Uninstalling Page
; Achievement Shelf: restyled (see SHELF_INSTFILES_SHOW).
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.InstFilesShow
!insertmacro MUI_UNPAGE_INSTFILES
Function un.InstFilesShow
  !insertmacro SHELF_INSTFILES_SHOW "UNINSTALL" "Removing ${PRODUCTNAME}" "This only takes a moment."
FunctionEnd

; Achievement Shelf: 3. Done page
UninstPage custom un.PageDone
Function un.PageDone
  Call un.SkipIfPassive
  !insertmacro SHELF_PAGE
  !insertmacro SHELF_ART
  ${If} $DeleteAppDataCheckboxState = 1
    StrCpy $R0 "Your shelf data and settings were deleted too."
  ${Else}
    StrCpy $R0 "Your shelf data was kept, so reinstalling picks up where you left off."
  ${EndIf}
  !insertmacro SHELF_HEADING $ShelfTextX 30 40 "DONE" "Uninstalled" "${PRODUCTNAME} has been removed from this PC. $R0"
  !insertmacro SHELF_LAST_PAGE "Close"
  nsDialogs::Show
FunctionEnd

;Languages
; PageReinstall reads the misspelled `choowHowToInstall` as a fallback, and makensis warns
; about a language string that is read but never set, so default it to an empty string for
; every language. This runs before the language files are included, so a custom language
; file that only defines the old spelling overrides the default and the fallback shows its
; text. `0` as the language id means the language inserted right above.
; TODO: remove the `choowHowToInstall` fallback in a future major version
{{#each languages}}
!insertmacro MUI_LANGUAGE "{{this}}"
LangString choowHowToInstall 0 ""
{{/each}}
!insertmacro MUI_RESERVEFILE_LANGDLL
{{#each language_files}}
  !include "{{this}}"
{{/each}}

Function .onInit
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/NS" $NoShortcutMode
  ${IfNot} ${Errors}
    StrCpy $NoShortcutMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}

  !if "${DISPLAYLANGUAGESELECTOR}" == "true"
    !insertmacro MUI_LANGDLL_DISPLAY
  !endif

  !insertmacro SetContext

  ${If} $INSTDIR == "${PLACEHOLDER_INSTALL_DIR}"
    ; Set default install location
    !if "${INSTALLMODE}" == "perMachine"
      ${If} ${RunningX64}
        !if "${ARCH}" == "x64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else if "${ARCH}" == "arm64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else
          StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
        !endif
      ${Else}
        StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
      ${EndIf}
    !else if "${INSTALLMODE}" == "currentUser"
      StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"
    !endif

    Call RestorePreviousInstallLocation
  ${EndIf}

  ; Achievement Shelf: what the Options page starts with. Everything is on for a new install,
  ; and silent and passive installs always use these defaults. Over an existing install the
  ; page starts from what's there now. That's read here because choosing "uninstall before
  ; installing" on the reinstall page removes it before the Options page shows.
  StrCpy $DesktopShortcutState 1
  StrCpy $StartMenuShortcutState 1
  StrCpy $AutostartState 1
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "UninstallString"
  ${If} $0 != ""
  ${AndIfNot} ${Silent}
  ${AndIf} $PassiveMode <> 1
    ${IfNot} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
      StrCpy $DesktopShortcutState 0
    ${EndIf}
    ${IfNot} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    ${AndIfNot} ${FileExists} "$SMPROGRAMS\${STARTMENUFOLDER}\${PRODUCTNAME}.lnk"
      StrCpy $StartMenuShortcutState 0
    ${EndIf}
    ReadRegStr $0 HKCU "${AUTOSTART_RUNKEY}" "${PRODUCTNAME}"
    ${If} $0 == ""
      StrCpy $AutostartState 0
    ${EndIf}
  ${EndIf}


  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_INIT
  !endif

  ; Abort downgrades if they are disabled, in the modes that skip the reinstall page choice:
  ; silent, passive and update
  !if "${ALLOWDOWNGRADES}" == "false"
  ${If} ${Silent}
  ${OrIf} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    ; Compare with the same installation the reinstall page would: a WiX one first
    Call FindWixInstall
    ${If} $R6 != ""
      ReadRegStr $R0 HKLM "$R6" "DisplayVersion"
    ${Else}
      ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"
    ${EndIf}
    nsis_tauri_utils::SemverCompare "${VERSION}" $R0
    Pop $R0
    ; If downgrading
    ${If} $R0 = -1
      ${If} ${Silent}
        System::Call 'kernel32::AttachConsole(i -1)i.r0'
        ${If} $0 <> 0
          System::Call 'kernel32::GetStdHandle(i -11)i.r0'
          System::call 'kernel32::SetConsoleTextAttribute(i r0, i 0x0004)' ; set red color
          FileWrite $0 "$(silentDowngrades)"
        ${EndIf}
      ${Else}
        MessageBox MB_ICONSTOP "$(silentDowngrades)"
      ${EndIf}
      Abort
    ${EndIf}
  ${EndIf}
  !endif
FunctionEnd

Section WebView2
  ; Check if Webview2 is already installed and skip this section
  ${If} ${RunningX64}
    ReadRegStr $4 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${Else}
    ReadRegStr $4 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  ${If} $4 == ""
    ReadRegStr $4 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}

  ${If} $4 == ""
    ; Webview2 installation
    ;
    ; Skip if updating
    ${If} $UpdateMode <> 1
      !if "${INSTALLWEBVIEW2MODE}" == "downloadBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        DetailPrint "$(webview2Downloading)"
        NSISdl::download "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Pop $0
        ${If} $0 == "success"
          DetailPrint "$(webview2DownloadSuccess)"
        ${Else}
          DetailPrint "$(webview2DownloadError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "embedBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"
        Delete "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        Goto install_webview2
      !endif

      Goto webview2_done

      install_webview2:
        DetailPrint "$(installingWebview2)"
        ; $6 holds the path to the webview2 installer
        ExecWait "$6 ${WEBVIEW2INSTALLERARGS} /install" $1
        ${If} $1 = 0
          DetailPrint "$(webview2InstallSuccess)"
        ${Else}
          DetailPrint "$(webview2InstallError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
      webview2_done:
    ${EndIf}
  ${Else}
    !if "${MINIMUMWEBVIEW2VERSION}" != ""
      ${VersionCompare} "${MINIMUMWEBVIEW2VERSION}" "$4" $R0
      ${If} $R0 = 1
        update_webview:
          DetailPrint "$(installingWebview2)"
          ${If} ${RunningX64}
            ReadRegStr $R1 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate" "path"
          ${Else}
            ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 == ""
            ReadRegStr $R1 HKCU "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 != ""
            ; Chromium updater docs: https://source.chromium.org/chromium/chromium/src/+/main:docs/updater/user_manual.md
            ; Modified from "HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Microsoft EdgeWebView\ModifyPath"
            ExecWait `"$R1" /install appguid=${WEBVIEW2APPGUID}&needsadmin=true` $1
            ${If} $1 = 0
              DetailPrint "$(webview2InstallSuccess)"
            ${Else}
              MessageBox MB_ICONEXCLAMATION|MB_ABORTRETRYIGNORE "$(webview2InstallError)" IDIGNORE ignore IDRETRY update_webview
              Quit
              ignore:
            ${EndIf}
          ${EndIf}
      ${EndIf}
    !endif
  ${EndIf}
SectionEnd

Section Install
  SetOutPath $INSTDIR

  !ifmacrodef NSIS_HOOK_PREINSTALL
    !insertmacro NSIS_HOOK_PREINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Copy main executable
  File "${MAINBINARYSRCPATH}"

  ; Copy resources
  {{#each resources_dirs}}
    CreateDirectory "$INSTDIR\\{{this}}"
  {{/each}}
  {{#each resources}}
    File /a "/oname={{this.[1]}}" "{{no-escape @key}}"
  {{/each}}

  ; Copy external binaries
  {{#each binaries}}
    File /a "/oname={{this}}" "{{no-escape @key}}"
  {{/each}}

  ; Create file associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
       !insertmacro APP_ASSOCIATE "{{ext}}" "{{or association.name ext}}" "{{association-description association.description ext}}" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0" "Open with ${PRODUCTNAME}" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
    {{/each}}
  {{/each}}

  ; Register deep links
  {{#each deep_link_protocols as |protocol| ~}}
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "URL Protocol" ""
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "" "URL:${BUNDLEID} protocol"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  {{/each}}

  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Save $INSTDIR in registry for future installations
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR

  !if "${INSTALLMODE}" == "both"
    ; Save install mode to be selected by default for the next installation such as updating
    ; or when uninstalling
    WriteRegStr SHCTX "${UNINSTKEY}" $MultiUser.InstallMode 1
  !endif

  ; Remove old main binary if it doesn't match new main binary name
  ReadRegStr $OldMainBinaryName SHCTX "${UNINSTKEY}" "MainBinaryName"
  ${If} $OldMainBinaryName != ""
  ${AndIf} $OldMainBinaryName != "${MAINBINARYNAME}.exe"
    Delete "$INSTDIR\$OldMainBinaryName"
  ${EndIf}

  ; Save current MAINBINARYNAME for future updates
  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}.exe"

  ; Registry information for add/remove programs
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoModify" "1"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" "1"

  ${GetSize} "$INSTDIR" "/M=uninstall.exe /S=0K /G=0" $0 $1 $2
  IntOp $0 $0 + ${ESTIMATEDSIZE}
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD SHCTX "${UNINSTKEY}" "EstimatedSize" "$0"

  !if "${HOMEPAGE}" != ""
    WriteRegStr SHCTX "${UNINSTKEY}" "URLInfoAbout" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "URLUpdateInfo" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "HelpLink" "${HOMEPAGE}"
  !endif

  ; Create start menu shortcut
  ; Achievement Shelf: unless unticked on the Options page (always ticked when silent or passive).
  ${If} $StartMenuShortcutState = 1
    !insertmacro MUI_STARTMENU_WRITE_BEGIN Application
      Call CreateOrUpdateStartMenuShortcut
    !insertmacro MUI_STARTMENU_WRITE_END
  ${EndIf}

  ; Create desktop shortcut for silent and passive installers
  ; because finish page will be skipped
  ; Achievement Shelf: and for the Options page's toggle, which replaced the finish page's.
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
  ${OrIf} $DesktopShortcutState = 1
    Call CreateOrUpdateDesktopShortcut
  ${EndIf}

  ; Achievement Shelf: the Options page's start with Windows toggle. Silent and passive installs,
  ; which the in-app updater uses, leave the setting as it is.
  ${If} $PassiveMode <> 1
  ${AndIfNot} ${Silent}
    ${If} $AutostartState = 1
      WriteRegStr HKCU "${AUTOSTART_RUNKEY}" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe"'
      ; Clears a "Disabled" set in Task Manager; with no value there, the entry counts as enabled.
      DeleteRegValue HKCU "${AUTOSTART_APPROVEDKEY}" "${PRODUCTNAME}"
    ${Else}
      DeleteRegValue HKCU "${AUTOSTART_RUNKEY}" "${PRODUCTNAME}"
    ${EndIf}
  ${EndIf}

  !ifmacrodef NSIS_HOOK_POSTINSTALL
    !insertmacro NSIS_HOOK_POSTINSTALL
  !endif

  ; Auto close this page for passive mode
  ; Achievement Shelf: and always, so the wizard moves on to the finish page. A failed
  ; install stays on this page and shows its log (see .onInstFailed).
  SetAutoClose true
SectionEnd

; Achievement Shelf: the progress page hides its log; show it when the install fails.
Function .onInstFailed
  !insertmacro SHELF_SHOW_LOG
FunctionEnd

Function .onInstSuccess
  ; Check for `/R` flag only in silent and passive installers because
  ; GUI installer has a toggle for the user to (re)start the app
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    ${GetOptions} $CMDLINE "/R" $R0
    ${IfNot} ${Errors}
      ${GetOptions} $CMDLINE "/ARGS" $R0
      nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "$R0"
    ${EndIf}
  ${EndIf}
FunctionEnd

Function un.onInit
  !insertmacro SetContext

  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_UNINIT
  !endif

  !insertmacro MUI_UNGETLANGUAGE

  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
FunctionEnd

Section Uninstall

  !ifmacrodef NSIS_HOOK_PREUNINSTALL
    !insertmacro NSIS_HOOK_PREUNINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Delete the app directory and its content from disk
  ; Copy main executable
  Delete "$INSTDIR\${MAINBINARYNAME}.exe"

  ; Delete resources
  {{#each resources}}
    Delete "$INSTDIR\\{{this.[1]}}"
  {{/each}}

  ; Delete external binaries
  {{#each binaries}}
    Delete "$INSTDIR\\{{this}}"
  {{/each}}

  ; Delete app associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
      !insertmacro APP_UNASSOCIATE "{{ext}}" "{{or association.name ext}}"
    {{/each}}
  {{/each}}

  ; Delete deep links
  {{#each deep_link_protocols as |protocol| ~}}
    ReadRegStr $R7 SHCTX "Software\Classes\\{{protocol}}\shell\open\command" ""
    ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
      DeleteRegKey SHCTX "Software\Classes\\{{protocol}}"
    ${EndIf}
  {{/each}}


  ; Delete uninstaller
  Delete "$INSTDIR\uninstall.exe"

  {{#each resources_ancestors}}
  RMDir /REBOOTOK "$INSTDIR\\{{this}}"
  {{/each}}
  RMDir "$INSTDIR"

  ; Remove shortcuts if not updating
  ${If} $UpdateMode <> 1
    !insertmacro DeleteAppUserModelId

    ; Remove start menu shortcut
    !insertmacro MUI_STARTMENU_GETFOLDER Application $AppStartMenuFolder
    !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      RMDir "$SMPROGRAMS\$AppStartMenuFolder"
    ${EndIf}
    !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    ${EndIf}

    ; Remove desktop shortcuts
    !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$DESKTOP\${PRODUCTNAME}.lnk"
      Delete "$DESKTOP\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}

  ; Remove registry information for add/remove programs
  !if "${INSTALLMODE}" == "both"
    DeleteRegKey SHCTX "${UNINSTKEY}"
  !else if "${INSTALLMODE}" == "perMachine"
    DeleteRegKey HKLM "${UNINSTKEY}"
  !else
    DeleteRegKey HKCU "${UNINSTKEY}"
  !endif

  ; Removes the Autostart entry for ${PRODUCTNAME} from the HKCU Run key if it exists.
  ; This ensures the program does not launch automatically after uninstallation if it exists.
  ; If it doesn't exist, it does nothing.
  ; We do this when not updating (to preserve the registry value on updates)
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  ${EndIf}

  ; Delete app data if the checkbox is selected
  ; and if not updating
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ; Clear the install location $INSTDIR from registry
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"

    ; Clear the install language from registry
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"

    SetShellVarContext current
    RmDir /r "$APPDATA\${BUNDLEID}"
    RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}

  !ifmacrodef NSIS_HOOK_POSTUNINSTALL
    !insertmacro NSIS_HOOK_POSTUNINSTALL
  !endif

  ; Auto close if passive mode or updating
  ; Achievement Shelf: and always, so the wizard moves on to the Done page. A failed uninstall
  ; stays on this page and shows its log (see un.onUninstFailed).
  SetAutoClose true
SectionEnd

; Achievement Shelf: the progress page hides its log; show it when the uninstall fails.
Function un.onUninstFailed
  !insertmacro SHELF_SHOW_LOG
FunctionEnd

Function RestorePreviousInstallLocation
  ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
  StrCmp $4 "" +2 0
    StrCpy $INSTDIR $4
FunctionEnd

Function Skip
  Abort
FunctionEnd

Function SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd
Function un.SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd

Function CreateOrUpdateStartMenuShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  StrCpy $R0 0

  !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  ${If} $R0 = 1
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  !if "${STARTMENUFOLDER}" != ""
    CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
    CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  !else
    CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !endif
FunctionEnd

Function CreateOrUpdateDesktopShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
FunctionEnd
