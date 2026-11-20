// Headless stand-in for the WinForms TreeView a classic settings window builds (CurtisBlack's Effects Lab shows a
// tree of effects). It holds the node tree and its properties so the dialog constructs and the dialog bridge can
// read it; nothing is painted. The API is the surface Effects Lab drives (nodes, selection, image list, sorting,
// the After* events); events never fire here, there being no interaction.
using System;
using System.Collections;
using System.Collections.Generic;

namespace System.Windows.Forms
{
    public class TreeNode
    {
        public TreeNode() => Nodes = new TreeNodeCollection(this);
        public TreeNode(string text) : this() => Text = text;
        public TreeNode(string text, TreeNode[] children) : this(text) => Nodes.AddRange(children);
        public TreeNode(string text, int imageIndex, int selectedImageIndex) : this()
            { Text = text; ImageIndex = imageIndex; SelectedImageIndex = selectedImageIndex; }

        public string Text { get; set; } = "";
        public string Name { get; set; } = "";
        public object? Tag { get; set; }
        public int ImageIndex { get; set; }
        public int SelectedImageIndex { get; set; }
        public bool IsExpanded { get; private set; }
        public TreeNode? Parent { get; internal set; }
        public TreeNodeCollection Nodes { get; }
        public System.Drawing.Font? NodeFont { get; set; }
        internal TreeView? treeView;
        public TreeView? TreeView => treeView ?? Parent?.TreeView;
        /// ponytail: nodes aren't laid out in the headless host, so there is no on-screen rectangle.
        public System.Drawing.Rectangle Bounds => System.Drawing.Rectangle.Empty;

        public void Expand() => IsExpanded = true;
        public void Collapse() => IsExpanded = false;
        public void ExpandAll() { IsExpanded = true; foreach (TreeNode n in Nodes) n.ExpandAll(); }
        public void EnsureVisible() { }
        public object Clone()
        {
            var n = new TreeNode(Text) { Name = Name, Tag = Tag, ImageIndex = ImageIndex, SelectedImageIndex = SelectedImageIndex, NodeFont = NodeFont };
            foreach (TreeNode child in Nodes) n.Nodes.Add((TreeNode)child.Clone());
            return n;
        }
        // ponytail: the host neither serialises nor deserialises nodes; present so a plug-in that overrides or calls
        // them binds.
        protected virtual void Serialize(System.Runtime.Serialization.SerializationInfo si, System.Runtime.Serialization.StreamingContext context) { }
        protected virtual void Deserialize(System.Runtime.Serialization.SerializationInfo si, System.Runtime.Serialization.StreamingContext context) { }
    }

    public class TreeNodeCollection : IEnumerable<TreeNode>
    {
        private readonly List<TreeNode> items = new();
        private readonly TreeNode? owner;
        internal TreeView? treeView;
        internal TreeNodeCollection(TreeNode? owner) => this.owner = owner;

        public int Count => items.Count;
        public TreeNode this[int index] => items[index];
        public TreeNode? this[string key] => items.Find(n => string.Equals(n.Name, key, StringComparison.Ordinal));
        public int Add(TreeNode node) { node.Parent = owner; node.treeView = treeView ?? owner?.treeView; items.Add(node); return items.Count - 1; }
        public TreeNode Add(string text) { var n = new TreeNode(text); Add(n); return n; }
        public void AddRange(TreeNode[] nodes) { foreach (TreeNode n in nodes) Add(n); }
        public void Clear() => items.Clear();
        public bool ContainsKey(string key) => this[key] is not null;
        public void Remove(TreeNode node) => items.Remove(node);
        public void RemoveAt(int index) => items.RemoveAt(index);
        // Real WinForms' TreeNodeCollection implements only the non-generic IEnumerable, so a plug-in's IL binds
        // GetEnumerator to one returning IEnumerator; the generic form stays for LINQ/foreach typing.
        public IEnumerator GetEnumerator() => items.GetEnumerator();
        IEnumerator<TreeNode> IEnumerable<TreeNode>.GetEnumerator() => items.GetEnumerator();
        internal void Sort(IComparer? cmp) { if (cmp is not null) items.Sort((a, b) => cmp.Compare(a, b)); foreach (TreeNode n in items) n.Nodes.Sort(cmp); }
    }

    public class TreeView : Control
    {
        public TreeNodeCollection Nodes { get; }
        public TreeView() => Nodes = new TreeNodeCollection(null) { treeView = this };

        public TreeNode? SelectedNode { get; set; }
        public TreeNode? TopNode { get; set; }
        public ImageList? ImageList { get; set; }
        public bool HideSelection { get; set; } = true;
        public string PathSeparator { get; set; } = "\\";
        public IComparer? TreeViewNodeSorter { get; set; }
        public int ItemHeight { get; set; } = 16;
        public bool ShowLines { get; set; } = true;
        public TreeViewDrawMode DrawMode { get; set; } = TreeViewDrawMode.Normal;

        public void Sort() => Nodes.Sort(TreeViewNodeSorter);
        public void ExpandAll() { foreach (TreeNode n in Nodes) n.ExpandAll(); }
        public void CollapseAll() { foreach (TreeNode n in Nodes) n.Collapse(); }
        public void BeginUpdate() { }
        public void EndUpdate() { }

        public event TreeViewEventHandler? AfterSelect;
        public event TreeViewEventHandler? AfterExpand;
        public event TreeViewEventHandler? AfterCollapse;
        public event TreeViewCancelEventHandler? BeforeExpand;
        public event TreeViewCancelEventHandler? BeforeSelect;
        public event DrawTreeNodeEventHandler? DrawNode;
        protected void OnAfterSelect(TreeViewEventArgs e) => AfterSelect?.Invoke(this, e);
        protected void OnAfterExpand(TreeViewEventArgs e) => AfterExpand?.Invoke(this, e);
        protected void OnAfterCollapse(TreeViewEventArgs e) => AfterCollapse?.Invoke(this, e);
        protected internal virtual void OnBeforeExpand(TreeViewCancelEventArgs e) => BeforeExpand?.Invoke(this, e);
        protected virtual void OnBeforeSelect(TreeViewCancelEventArgs e) => BeforeSelect?.Invoke(this, e);
        protected virtual void OnDrawNode(DrawTreeNodeEventArgs e) => DrawNode?.Invoke(this, e);
    }

    public enum TreeViewDrawMode { Normal, OwnerDrawText, OwnerDrawAll }

    public class TreeViewCancelEventArgs : System.ComponentModel.CancelEventArgs
    {
        public TreeViewCancelEventArgs(TreeNode? node, bool cancel, TreeViewAction action) : base(cancel) { Node = node; Action = action; }
        public TreeNode? Node { get; }
        public TreeViewAction Action { get; }
    }

    public delegate void TreeViewCancelEventHandler(object? sender, TreeViewCancelEventArgs e);

    public class DrawTreeNodeEventArgs : EventArgs
    {
        public DrawTreeNodeEventArgs(System.Drawing.Graphics graphics, TreeNode? node, System.Drawing.Rectangle bounds, TreeNodeStates state)
            { Graphics = graphics; Node = node; Bounds = bounds; State = state; }
        public System.Drawing.Graphics Graphics { get; }
        public TreeNode? Node { get; }
        public System.Drawing.Rectangle Bounds { get; }
        public TreeNodeStates State { get; }
        public bool DrawDefault { get; set; }
    }

    public delegate void DrawTreeNodeEventHandler(object? sender, DrawTreeNodeEventArgs e);

    [Flags]
    public enum TreeNodeStates
    {
        Checked = 8, Default = 32, Focused = 16, Grayed = 2, Hot = 64, Indeterminate = 256,
        Marked = 128, Selected = 1, ShowKeyboardCues = 512,
    }

    public enum TreeViewAction { Unknown, ByKeyboard, ByMouse, Collapse, Expand }

    public class TreeViewEventArgs : EventArgs
    {
        public TreeViewEventArgs(TreeNode? node) => Node = node;
        public TreeViewEventArgs(TreeNode? node, TreeViewAction action) { Node = node; Action = action; }
        public TreeNode? Node { get; }
        public TreeViewAction Action { get; }
    }

    public delegate void TreeViewEventHandler(object? sender, TreeViewEventArgs e);
}
