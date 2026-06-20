package platform

import (
	"runtime"
)

func OS() string {
	return runtime.GOOS
}

func Arch() string {
	return runtime.GOARCH
}

func IsLinux() bool {
	return runtime.GOOS == "linux"
}

func IsDarwin() bool {
	return runtime.GOOS == "darwin"
}
