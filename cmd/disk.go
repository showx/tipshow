package cmd

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/shengsheng/tipshow/internal/collect"
	"github.com/shengsheng/tipshow/internal/output"
	"github.com/shengsheng/tipshow/internal/runner"
	"github.com/shengsheng/tipshow/internal/tools"
	"github.com/spf13/cobra"
)

var (
	diskDuPath   string
	diskDuDepth  int
	diskDuTop    int
)

var diskCmd = &cobra.Command{
	Use:   "disk",
	Short: "磁盘：df 概览 / du 一级目录占用",
}

var diskDfCmd = &cobra.Command{
	Use:   "df",
	Short: "文件系统占用（优先 df，fallback gopsutil）",
	Run: func(cmd *cobra.Command, args []string) {
		if tools.Available("df") {
			argv := []string{"df", "-h"}
			if len(args) > 0 {
				argv = append(argv, args...)
			}
			if output.JSONMode {
				res, err := runner.Run(context.Background(), argv, 15*time.Second)
				if err != nil {
					fail(err)
				}
				_ = output.PrintJSON(map[string]any{"tool": "df", "stdout": res.Stdout, "stderr": res.Stderr})
				return
			}
			if err := runner.RunOrPrint(context.Background(), argv, 15*time.Second); err != nil {
				fail(err)
			}
			return
		}

		usages, err := collect.DiskAll()
		if err != nil {
			fail(err)
		}
		if err := output.Emit(usages, func() string {
			var b strings.Builder
			for _, u := range usages {
				fmt.Fprintf(&b, "%-20s %8s / %-8s (%5.1f%%) %s\n",
					u.Path, collect.FormatBytes(u.Used), collect.FormatBytes(u.Total), u.UsedPercent, u.Fstype)
			}
			return strings.TrimRight(b.String(), "\n")
		}); err != nil {
			fail(err)
		}
	},
}

type duEntry struct {
	Name  string `json:"name"`
	Path  string `json:"path"`
	Bytes uint64 `json:"bytes"`
	Human string `json:"human"`
}

var diskDuCmd = &cobra.Command{
	Use:   "du",
	Short: "一级子目录空间占用（du -d 1 或 gopsutil 扫描）",
	Run: func(cmd *cobra.Command, args []string) {
		root := diskDuPath
		if len(args) > 0 {
			root = args[0]
		}
		abs, err := filepath.Abs(root)
		if err != nil {
			fail(err)
		}

		if tools.Available("du") {
			argv := []string{"du", "-h", "-d", fmt.Sprintf("%d", diskDuDepth), abs}
			if output.JSONMode {
				res, err := runner.Run(context.Background(), argv, 120*time.Second)
				if err != nil {
					fail(err)
				}
				_ = output.PrintJSON(map[string]any{"tool": "du", "path": abs, "stdout": res.Stdout})
				return
			}
			if err := runner.RunOrPrint(context.Background(), argv, 120*time.Second); err != nil {
				fail(err)
			}
			return
		}

		entries, err := scanDirSizes(abs, diskDuTop)
		if err != nil {
			fail(err)
		}
		if err := output.Emit(entries, func() string {
			var b strings.Builder
			fmt.Fprintf(&b, "路径: %s\n", abs)
			for _, e := range entries {
				fmt.Fprintf(&b, "%8s  %s\n", e.Human, e.Name)
			}
			return strings.TrimRight(b.String(), "\n")
		}); err != nil {
			fail(err)
		}
	},
}

func scanDirSizes(root string, topN int) ([]duEntry, error) {
	children, err := os.ReadDir(root)
	if err != nil {
		return nil, err
	}
	var entries []duEntry
	for _, c := range children {
		if !c.IsDir() {
			continue
		}
		p := filepath.Join(root, c.Name())
		size, err := dirSize(p)
		if err != nil {
			continue
		}
		entries = append(entries, duEntry{
			Name:  c.Name(),
			Path:  p,
			Bytes: size,
			Human: collect.FormatBytes(size),
		})
	}
	sort.Slice(entries, func(i, j int) bool { return entries[i].Bytes > entries[j].Bytes })
	if topN > 0 && len(entries) > topN {
		entries = entries[:topN]
	}
	return entries, nil
}

func dirSize(path string) (uint64, error) {
	var total uint64
	err := filepath.WalkDir(path, func(p string, d os.DirEntry, err error) error {
		if err != nil {
			return nil
		}
		if d.IsDir() {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return nil
		}
		if info.Mode().IsRegular() {
			total += uint64(info.Size())
		}
		return nil
	})
	return total, err
}

func init() {
	diskDuCmd.Flags().StringVarP(&diskDuPath, "path", "p", ".", "扫描根目录")
	diskDuCmd.Flags().IntVarP(&diskDuDepth, "depth", "d", 1, "du 深度")
	diskDuCmd.Flags().IntVarP(&diskDuTop, "top", "n", 20, "gopsutil fallback 时显示前 N 项")
	diskCmd.AddCommand(diskDfCmd)
	diskCmd.AddCommand(diskDuCmd)
}
