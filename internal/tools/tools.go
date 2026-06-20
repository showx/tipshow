package tools

import (
	"os/exec"

	"github.com/shengsheng/tipshow/internal/platform"
)

type Level string

const (
	LevelBasic         Level = "basic"
	LevelIntermediate  Level = "intermediate"
	LevelAdvanced      Level = "advanced"
)

type Spec struct {
	Name             string
	Level            Level
	Description      string
	InstallHintLinux string
	InstallHintMacOS string
}

var Catalog = []Spec{
	// 基础层
	{Name: "gopsutil", Level: LevelBasic, Description: "Go 内置跨平台采集 (CPU/内存/磁盘/网络)"},
	{Name: "df", Level: LevelBasic, Description: "文件系统磁盘占用", InstallHintLinux: "coreutils", InstallHintMacOS: "内置"},
	{Name: "du", Level: LevelBasic, Description: "目录空间占用", InstallHintLinux: "coreutils", InstallHintMacOS: "内置"},
	// 中级
	{Name: "htop", Level: LevelIntermediate, Description: "交互式进程/CPU 视图", InstallHintLinux: "htop", InstallHintMacOS: "brew install htop"},
	{Name: "iostat", Level: LevelIntermediate, Description: "CPU/磁盘 IO 统计", InstallHintLinux: "sysstat", InstallHintMacOS: "内置"},
	{Name: "iotop", Level: LevelIntermediate, Description: "按进程磁盘 IO", InstallHintLinux: "iotop", InstallHintMacOS: "brew install iotop"},
	{Name: "ifconfig", Level: LevelIntermediate, Description: "网络接口 (legacy)", InstallHintLinux: "net-tools", InstallHintMacOS: "内置"},
	{Name: "ip", Level: LevelIntermediate, Description: "现代网络配置", InstallHintLinux: "iproute2", InstallHintMacOS: "brew install iproute2mac"},
	{Name: "ss", Level: LevelIntermediate, Description: "Socket 连接", InstallHintLinux: "iproute2", InstallHintMacOS: "不可用，用 netstat/lsof"},
	{Name: "netstat", Level: LevelIntermediate, Description: "网络连接/端口", InstallHintLinux: "net-tools", InstallHintMacOS: "内置"},
	{Name: "lsof", Level: LevelIntermediate, Description: "打开文件/网络连接", InstallHintLinux: "lsof", InstallHintMacOS: "内置"},
	// 高级
	{Name: "sar", Level: LevelAdvanced, Description: "系统活动报告", InstallHintLinux: "sysstat", InstallHintMacOS: "Linux sysstat 为主"},
	{Name: "pidstat", Level: LevelAdvanced, Description: "按 PID 的 CPU/IO/内存", InstallHintLinux: "sysstat", InstallHintMacOS: "Linux sysstat"},
	{Name: "perf", Level: LevelAdvanced, Description: "内核/CPU 性能分析", InstallHintLinux: "linux-tools-common", InstallHintMacOS: "仅 Linux"},
	{Name: "biotop", Level: LevelAdvanced, Description: "块设备 IO top", InstallHintLinux: "biotop", InstallHintMacOS: "Linux / 源码"},
	{Name: "blktrace", Level: LevelAdvanced, Description: "块层 IO 追踪", InstallHintLinux: "blktrace", InstallHintMacOS: "仅 Linux"},
	{Name: "tcptop", Level: LevelAdvanced, Description: "按连接 TCP 流量", InstallHintLinux: "bcc / libbpf-tools", InstallHintMacOS: "Linux bcc"},
	{Name: "tcpdump", Level: LevelAdvanced, Description: "网络抓包", InstallHintLinux: "tcpdump", InstallHintMacOS: "brew install tcpdump"},
}

func Which(name string) string {
	path, err := exec.LookPath(name)
	if err != nil {
		return ""
	}
	return path
}

func Available(name string) bool {
	if name == "gopsutil" {
		return true
	}
	return Which(name) != ""
}

func Resolve(candidates ...string) string {
	for _, c := range candidates {
		if p := Which(c); p != "" {
			return p
		}
	}
	return ""
}

type ReportRow struct {
	Name        string `json:"name"`
	Level       string `json:"level"`
	Description string `json:"description"`
	Available   bool   `json:"available"`
	Path        string `json:"path"`
	InstallHint string `json:"install_hint,omitempty"`
}

func AvailabilityReport() []ReportRow {
	rows := make([]ReportRow, 0, len(Catalog))
	for _, spec := range Catalog {
		avail := Available(spec.nameKey())
		path := Which(spec.nameKey())
		if spec.Name == "gopsutil" {
			path = "builtin"
		}
		hint := spec.InstallHintLinux
		if platform.IsDarwin() {
			hint = spec.InstallHintMacOS
		}
		if !avail {
			rows = append(rows, ReportRow{
				Name:        spec.Name,
				Level:       string(spec.Level),
				Description: spec.Description,
				Available:   false,
				Path:        path,
				InstallHint: hint,
			})
		} else {
			rows = append(rows, ReportRow{
				Name:        spec.Name,
				Level:       string(spec.Level),
				Description: spec.Description,
				Available:   true,
				Path:        path,
			})
		}
	}
	return rows
}

func (s Spec) nameKey() string {
	return s.Name
}
