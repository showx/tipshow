package cmd

import (
	"fmt"
	"strings"
	"time"

	"github.com/shengsheng/tipshow/internal/collect"
	"github.com/shengsheng/tipshow/internal/output"
	"github.com/spf13/cobra"
)

var cpuInterval float64

var cpuCmd = &cobra.Command{
	Use:   "cpu",
	Short: "查看 CPU 信息和使用率（基础层，内置 gopsutil）",
	Run: func(cmd *cobra.Command, args []string) {
		info, err := collect.CPU(time.Duration(cpuInterval * float64(time.Second)))
		if err != nil {
			fail(err)
		}
		if err := output.Emit(info, func() string {
			var b strings.Builder
			fmt.Fprintf(&b, "型号: %s\n", info.Model)
			fmt.Fprintf(&b, "核心: %d 逻辑 / %d 物理, %.0f MHz\n", info.LogicalCores, info.PhysicalCores, info.Mhz)
			fmt.Fprintf(&b, "总使用率: %.1f%%\n", info.UsageTotal)
			for i, u := range info.UsagePercent {
				fmt.Fprintf(&b, "  CPU%-2d: %.1f%%\n", i, u)
			}
			return strings.TrimRight(b.String(), "\n")
		}); err != nil {
			fail(err)
		}
	},
}

func init() {
	cpuCmd.Flags().Float64VarP(&cpuInterval, "interval", "i", 1, "采样间隔（秒）")
}
