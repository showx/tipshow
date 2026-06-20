package collect

import (
	"fmt"
	"time"

	"github.com/shirou/gopsutil/v4/cpu"
	"github.com/shirou/gopsutil/v4/disk"
	"github.com/shirou/gopsutil/v4/host"
	"github.com/shirou/gopsutil/v4/load"
	"github.com/shirou/gopsutil/v4/mem"
	"github.com/shirou/gopsutil/v4/net"
)

type CPUInfo struct {
	LogicalCores  int       `json:"logical_cores"`
	PhysicalCores int       `json:"physical_cores"`
	Model         string    `json:"model"`
	Mhz           float64   `json:"mhz"`
	UsagePercent  []float64 `json:"usage_percent_per_cpu"`
	UsageTotal    float64   `json:"usage_total_percent"`
}

type MemInfo struct {
	Total       uint64  `json:"total_bytes"`
	Available   uint64  `json:"available_bytes"`
	Used        uint64  `json:"used_bytes"`
	UsedPercent float64 `json:"used_percent"`
	SwapTotal   uint64  `json:"swap_total_bytes"`
	SwapUsed    uint64  `json:"swap_used_bytes"`
}

type HostInfo struct {
	Hostname        string    `json:"hostname"`
	OS              string    `json:"os"`
	Platform        string    `json:"platform"`
	PlatformVersion string    `json:"platform_version"`
	Kernel          string    `json:"kernel"`
	Uptime          uint64    `json:"uptime_seconds"`
	BootTime        time.Time `json:"boot_time"`
	Load1           float64   `json:"load1"`
	Load5           float64   `json:"load5"`
	Load15          float64   `json:"load15"`
}

type DiskUsage struct {
	Path        string  `json:"path"`
	Fstype      string  `json:"fstype"`
	Total       uint64  `json:"total_bytes"`
	Used        uint64  `json:"used_bytes"`
	Free        uint64  `json:"free_bytes"`
	UsedPercent float64 `json:"used_percent"`
}

type NetCounter struct {
	Name        string `json:"name"`
	BytesSent   uint64 `json:"bytes_sent"`
	BytesRecv   uint64 `json:"bytes_recv"`
	PacketsSent uint64 `json:"packets_sent"`
	PacketsRecv uint64 `json:"packets_recv"`
	Errin       uint64 `json:"errin"`
	Errout      uint64 `json:"errout"`
	Dropin      uint64 `json:"dropin"`
	Dropout     uint64 `json:"dropout"`
}

func CPU(interval time.Duration) (CPUInfo, error) {
	info, err := cpu.Info()
	if err != nil {
		return CPUInfo{}, err
	}
	perCPU, err := cpu.Percent(interval, true)
	if err != nil {
		return CPUInfo{}, err
	}
	total, err := cpu.Percent(interval, false)
	if err != nil {
		return CPUInfo{}, err
	}
	logical, _ := cpu.Counts(true)
	physical, _ := cpu.Counts(false)

	model := ""
	var mhz float64
	if len(info) > 0 {
		model = info[0].ModelName
		mhz = info[0].Mhz
	}

	usageTotal := 0.0
	if len(total) > 0 {
		usageTotal = total[0]
	}

	return CPUInfo{
		LogicalCores:  logical,
		PhysicalCores: physical,
		Model:         model,
		Mhz:           mhz,
		UsagePercent:  perCPU,
		UsageTotal:    usageTotal,
	}, nil
}

func Memory() (MemInfo, error) {
	v, err := mem.VirtualMemory()
	if err != nil {
		return MemInfo{}, err
	}
	s, err := mem.SwapMemory()
	if err != nil {
		return MemInfo{
			Total:       v.Total,
			Available:   v.Available,
			Used:        v.Used,
			UsedPercent: v.UsedPercent,
		}, nil
	}
	return MemInfo{
		Total:       v.Total,
		Available:   v.Available,
		Used:        v.Used,
		UsedPercent: v.UsedPercent,
		SwapTotal:   s.Total,
		SwapUsed:    s.Used,
	}, nil
}

func Host() (HostInfo, error) {
	h, err := host.Info()
	if err != nil {
		return HostInfo{}, err
	}
	l, err := load.Avg()
	if err != nil {
		return HostInfo{
			Hostname:        h.Hostname,
			OS:              h.OS,
			Platform:        h.Platform,
			PlatformVersion: h.PlatformVersion,
			Kernel:          h.KernelVersion,
			Uptime:          h.Uptime,
			BootTime:        time.Unix(int64(h.BootTime), 0),
		}, nil
	}
	return HostInfo{
		Hostname:        h.Hostname,
		OS:              h.OS,
		Platform:        h.Platform,
		PlatformVersion: h.PlatformVersion,
		Kernel:          h.KernelVersion,
		Uptime:          h.Uptime,
		BootTime:        time.Unix(int64(h.BootTime), 0),
		Load1:           l.Load1,
		Load5:           l.Load5,
		Load15:          l.Load15,
	}, nil
}

func DiskAll() ([]DiskUsage, error) {
	parts, err := disk.Partitions(false)
	if err != nil {
		return nil, err
	}
	out := make([]DiskUsage, 0, len(parts))
	for _, p := range parts {
		u, err := disk.Usage(p.Mountpoint)
		if err != nil {
			continue
		}
		out = append(out, DiskUsage{
			Path:        p.Mountpoint,
			Fstype:      p.Fstype,
			Total:       u.Total,
			Used:        u.Used,
			Free:        u.Free,
			UsedPercent: u.UsedPercent,
		})
	}
	return out, nil
}

func NetIO() ([]NetCounter, error) {
	counters, err := net.IOCounters(true)
	if err != nil {
		return nil, err
	}
	out := make([]NetCounter, 0, len(counters))
	for _, c := range counters {
		out = append(out, NetCounter{
			Name:        c.Name,
			BytesSent:   c.BytesSent,
			BytesRecv:   c.BytesRecv,
			PacketsSent: c.PacketsSent,
			PacketsRecv: c.PacketsRecv,
			Errin:       c.Errin,
			Errout:      c.Errout,
			Dropin:      c.Dropin,
			Dropout:     c.Dropout,
		})
	}
	return out, nil
}

func FormatBytes(n uint64) string {
	const unit = 1024
	if n < unit {
		return fmt.Sprintf("%d B", n)
	}
	div, exp := uint64(unit), 0
	for v := n / unit; v >= unit; v /= unit {
		div *= unit
		exp++
	}
	return fmt.Sprintf("%.1f %ciB", float64(n)/float64(div), "KMGTPE"[exp])
}
