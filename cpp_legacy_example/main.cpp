#include <Windows.h>
#include <iostream>
#include <string>
#include <locale>
#include <codecvt>
#include <queue>

constexpr auto PROGRAM_NAME = L"ImeWatcher";
constexpr auto PROGRAM_WINDOW = L"ImeWatcherWindow";

HHOOK _keyboardHook;

class LanguageTracker {
private:
	std::queue<LANGID> _queue;

public:
	auto update(LANGID new_lang) {
		if (_queue.size() >= 2) {
			_queue.pop();
		}
		_queue.push(new_lang);
	}

	auto isChanged() const -> bool {
		return !_queue.empty() && (_queue.back() != _queue.front());
	}

	auto current() const -> LANGID {
		return _queue.empty() ? LANG_ENGLISH : _queue.back();
	}
};

class TrayIcon {
private:
	NOTIFYICONDATA _notifyIconData;
	HMENU _trayMenu;
	bool _isMinimized = false;
	HWND _hWnd;

public:
	TrayIcon(HWND hWnd) {
		_hWnd = hWnd;

		_notifyIconData.cbSize = sizeof(NOTIFYICONDATA);
		_notifyIconData.hWnd = hWnd;
		_notifyIconData.uID = 1;
		_notifyIconData.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
		_notifyIconData.uCallbackMessage = WM_USER + 1;
		_notifyIconData.hIcon = LoadIcon(GetModuleHandle(NULL), MAKEINTRESOURCE(IDI_APPLICATION));
		lstrcpy(_notifyIconData.szTip, PROGRAM_NAME);
		Shell_NotifyIcon(NIM_ADD, &_notifyIconData);

		_trayMenu = CreatePopupMenu();
		AppendMenu(_trayMenu, MF_STRING, 1, L"설정");
		AppendMenu(_trayMenu, MF_STRING, 2, L"종료");
	}

	auto minimize() {
		if (!_isMinimized) {
			Shell_NotifyIcon(NIM_ADD, &_notifyIconData);
			ShowWindow(_hWnd, SW_HIDE);
			_isMinimized = true;
		}
	}

	auto restore() {
		if (_isMinimized) {
			Shell_NotifyIcon(NIM_DELETE, &_notifyIconData);
			ShowWindow(_hWnd, SW_RESTORE);
			_isMinimized = false;
		}
	}

	auto popupMenu() {
		POINT pt;
		GetCursorPos(&pt);
		TrackPopupMenu(_trayMenu, TPM_RIGHTALIGN | TPM_BOTTOMALIGN, pt.x, pt.y, 0, _hWnd, NULL);
	}

	auto handleMessage(UINT message, LPARAM lParam) {
		if (message == _notifyIconData.uCallbackMessage) {
			switch (LOWORD(lParam)) {
			case WM_RBUTTONDOWN:
				popupMenu();
				break;
			case WM_LBUTTONDBLCLK:
				restore();
				break;
			default:
				break;
			}
		}
	}
};

auto sendImeChangedEventToKeyboard() {
	// TODO : 기능 구현
	return;
}

auto getKeyboardLayout() -> LANGID
{
	return PRIMARYLANGID(LOWORD(HandleToLong(GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), nullptr)))));
}

// 테스트, 디버깅 용도
auto getLangStringFrom(LANGID langID) -> std::string {
	LCID lcid = MAKELCID(langID, SORT_DEFAULT);
	std::wstring_convert<std::codecvt_utf8<wchar_t>> converter;

	WCHAR languageName[LOCALE_NAME_MAX_LENGTH];
	if (GetLocaleInfo(lcid, LOCALE_SLANGUAGE, languageName, LOCALE_NAME_MAX_LENGTH) > 0) {
		return converter.to_bytes(languageName);
	}
	else {
		return "Unknown Language";
	}
}

auto updateImeLang()
{
	constexpr auto IMC_GETOPENSTATUS = 0x0005;
	static LanguageTracker tracker;

	HWND hIME = ImmGetDefaultIMEWnd(GetForegroundWindow());
	LRESULT status = SendMessage(hIME, WM_IME_CONTROL, IMC_GETOPENSTATUS, 0);
	auto lang = getKeyboardLayout();

	switch (status)
	{
	case IME_CMODE_NATIVE:
		tracker.update(lang);
		break;
	case IME_CMODE_ALPHANUMERIC:
	default:
		tracker.update(LANG_ENGLISH);
		break;
	}

	if (tracker.isChanged()) {
		std::cout << getLangStringFrom(tracker.current()) << std::endl;
		sendImeChangedEventToKeyboard();
	}
};

LRESULT CALLBACK KeyboardProc(int nCode, WPARAM wParam, LPARAM lParam)
{
	if (nCode >= 0)
	{
		if (wParam == WM_KEYDOWN || wParam == WM_KEYUP)
		{
			updateImeLang();
		}
	}

	return CallNextHookEx(_keyboardHook, nCode, wParam, lParam);
}

VOID CALLBACK WinEventProcCallback(HWINEVENTHOOK hWinEventHook, DWORD dwEvent, HWND hwnd, LONG idObject, LONG idChild, DWORD dwEventThread, DWORD dwmsEventTime)
{
	HWND foregroundWindow = GetForegroundWindow();
	if (foregroundWindow) {
		std::cout << "창변경 감지" << std::endl;
		updateImeLang();
	}
}

LRESULT CALLBACK WndProc(HWND hWnd, UINT message, WPARAM wParam, LPARAM lParam)
{
	static TrayIcon tray{ hWnd };

	tray.handleMessage(message, lParam);

	switch (message)
	{
	case WM_CREATE:
#ifdef _DEBUG
		// 콘솔 창을 생성
		if (AllocConsole() != 0)
		{
			FILE* pFile = nullptr;
			if (freopen_s(&pFile, "CONOUT$", "w", stdout) != 0)
			{
				MessageBox(nullptr, L"Failed to reopen console output!", L"Error", MB_ICONERROR);
				FreeConsole();
			}
		}
#endif
		break;

	case WM_CLOSE:
		tray.minimize();
		break;

	case WM_INPUTLANGCHANGE:
		std::cout << "키보드 레이아웃 변경 감지" << std::endl;
		updateImeLang();
		break;

	case WM_SYSCOMMAND:
		switch (wParam)
		{
		case SC_MINIMIZE:
		case SC_CLOSE:
			tray.minimize();
			break;

		default:
			break;
		}
		break;
	case WM_COMMAND:
		// 트레이 메뉴에서의 명령 처리
		switch (LOWORD(wParam)) {
		case 1:
			// 설정 버튼
			tray.restore();
			break;
		case 2:
			// 종료 버튼
			DestroyWindow(hWnd);
			break;
		}
		break;

	case WM_DESTROY:
		FreeConsole();
		PostQuitMessage(0);
		break;

	default:
		return DefWindowProc(hWnd, message, wParam, lParam);
	}

	return 0;
}

int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPSTR lpCmdLine, int nCmdShow)
{
	// 윈도우 클래스 등록
	WNDCLASS wc = { 0 };
	wc.lpfnWndProc = WndProc;
	wc.hInstance = hInstance;
	wc.lpszClassName = PROGRAM_NAME;
	RegisterClass(&wc);

	// 윈도우 생성
	HWND hWnd = CreateWindow(PROGRAM_NAME, PROGRAM_WINDOW, WS_OVERLAPPEDWINDOW,
		CW_USEDEFAULT, CW_USEDEFAULT, 400, 300, nullptr, nullptr, hInstance, nullptr);

	_keyboardHook = SetWindowsHookEx(WH_KEYBOARD_LL, KeyboardProc, GetModuleHandle(nullptr), 0);
	auto hEvent = SetWinEventHook(EVENT_SYSTEM_FOREGROUND,
		EVENT_SYSTEM_FOREGROUND, nullptr,
		WinEventProcCallback, 0, 0,
		WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);

	updateImeLang();

	if (hWnd)
	{
		ShowWindow(hWnd, nCmdShow);

		MSG msg;
		while (GetMessage(&msg, nullptr, 0, 0))
		{
			TranslateMessage(&msg);
			DispatchMessage(&msg);
		}
	}

	UnhookWindowsHookEx(_keyboardHook);
	UnhookWinEvent(hEvent);

	return 0;
}
