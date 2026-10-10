<!-- Release Pipeline icon - 16x16
     最紧的尺寸。与 32px 的缩放关系相反：描边不再等比缩小，
     而是反向加粗到 2px（下限），否则 1.6px 的线在 16px 下会退化成灰糊。
     只保留最少的构成单元：
       - 两根柱子（阶段 2、3），丢弃阶段 1
       - 完整箭头，箭杆加长到 7 使尾端接到柱顶右侧，避免悬空
     构造法与主图一致。尖端 T=(13.5, 3.5)，描边 S=2，翼长 Lw=5，杆长 Ls=7。
     箭杆定位（绕中心顺时针 45°，上端中心落在 T）：
         left = T.x - 0.3536*Ls - S/2
         top  = T.y + 0.3536*Ls - Ls/2
     竖翼顶部比尖端上探 0.5px（=0.25*S）闭合接缝。 -->
<Snapshot type="png" background="transparent">
    <Container
        width="16" height="16"
        borderRadius="4"
        gradientType="LINEAR"
        gradientColors="#0B1B4D,#1E3A8A,#5B21B6"
        gradientStops="0,0.55,1"
        gradientBegin="TOP_LEFT"
        gradientEnd="BOTTOM_RIGHT">

        <Stack>

            <!-- 阶段 2：进行中 -->
            <Positioned left="2" bottom="2.5" width="3" height="4">
                <Container color="#22D3EEE6" borderRadius="1"/>
            </Positioned>

            <!-- 阶段 3：发布中 -->
            <Positioned left="6" bottom="2.5" width="3" height="6.5">
                <Container color="#67E8F9" borderRadius="1"/>
            </Positioned>

            <!-- 箭头横翼：从尖端向左 -->
            <Positioned left="8.5" top="2.5" width="5" height="2">
                <Container color="#F0F9FF" borderRadius="1"/>
            </Positioned>

            <!-- 箭头竖翼：从尖端向下，顶部上探 0.5px 闭合接缝 -->
            <Positioned left="12.5" top="3" width="2" height="6">
                <Container color="#F0F9FF" borderRadius="1"/>
            </Positioned>

            <!-- 箭杆：旋转 45°，上端落在尖端，尾端落到柱顶右侧 -->
            <Positioned left="9.53" top="3.47" width="2" height="7">
                <Transform
                    matrix="(0.7071,0.7071,0,0,-0.7071,0.7071,0,0,0,0,1,0,0,0,0,1)"
                    alignment="CENTER">
                    <Container width="2" height="7" color="#F0F9FF" borderRadius="1"/>
                </Transform>
            </Positioned>

        </Stack>
    </Container>
</Snapshot>