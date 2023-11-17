# MTTPN

## 依赖
### Boost
工具依赖于Boost库，随工具生成的boost静态库为win11下SDK的x64版本，安装地址https://www.boost.org/，编译器为Visual Studio 2022(Version 17.0-),安装地址https://learn.microsoft.com/zh-cn/visualstudio/releases/2022/redistribution
### Graphviz
任务依赖图和Petri网的可视化需要dot支持，安装参考 https://graphviz.org/download/ ,安装好后将可执行文件加入PATH路径


## 任务依赖关系
工具依次读取任务依赖图和任务属性描述

## 任务依赖图
可选择生成或不生成任务依赖图

## Petri网
在读取任务依赖关系后生成Petri网,自动输出并保存网的图形化结果在工具文件夹下

## 状态类图
计算WCET和检查死锁前需要先生成状态类图,若系统存在死锁，则WCET的计算可能存在错误


## 注
任务界面实现简单，没有进行异常处理，若读取错误的文件，则会崩溃，需重新启动，在生成一类网后需要重新启动工具读取新的配置文件。
