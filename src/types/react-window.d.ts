declare module "react-window" {
  import * as React from "react";
  import type { ComponentType, CSSProperties } from "react";

  export type GridChildComponentProps<T = unknown> = {
    columnIndex: number;
    rowIndex: number;
    style: CSSProperties;
    data: T;
    isScrolling?: boolean;
  };

  export type FixedSizeGridProps<T = unknown> = {
    children: ComponentType<GridChildComponentProps<T>>;
    className?: string;
    columnCount: number;
    columnWidth: number;
    height: number;
    itemData?: T;
    onScroll?: (position: {
      scrollLeft: number;
      scrollTop: number;
      horizontalScrollDirection: "forward" | "backward";
      verticalScrollDirection: "forward" | "backward";
      scrollUpdateWasRequested: boolean;
    }) => void;
    onItemsRendered?: (range: {
      overscanColumnStartIndex: number;
      overscanColumnStopIndex: number;
      overscanRowStartIndex: number;
      overscanRowStopIndex: number;
      visibleColumnStartIndex: number;
      visibleColumnStopIndex: number;
      visibleRowStartIndex: number;
      visibleRowStopIndex: number;
    }) => void;
    overscanColumnCount?: number;
    overscanRowCount?: number;
    rowCount: number;
    rowHeight: number;
    width: number;
  };

  export class FixedSizeGrid<T = unknown> extends React.Component<
    FixedSizeGridProps<T>
  > {
    scrollTo(params: { scrollLeft?: number; scrollTop?: number }): void;
    scrollToItem(params: {
      align?: "auto" | "smart" | "center" | "end" | "start";
      columnIndex?: number;
      rowIndex?: number;
    }): void;
  }
}
